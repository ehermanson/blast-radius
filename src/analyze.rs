use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::{Cli, Command};
use crate::fs::RepoContext;
use crate::graph::{AnalysisMode, AnalysisResult, AnalysisTarget, ModuleState};
use crate::parse::{ExportKind, ModuleFacts, ReexportTarget, parse_module};
use crate::resolve::{Resolution, Resolver};

mod walk;
use walk::{ConsumerLink, build_reverse_links, compute_root_impacts, run_bfs};

mod result;
use result::{ResultMetadata, build_result, collect_workspaces};

mod diagnostics;
use diagnostics::unresolved_import_diagnostics;

struct AnalysisData<'a> {
    context: &'a RepoContext,
    modules: &'a BTreeMap<PathBuf, ModuleFacts>,
    module_states: &'a BTreeMap<PathBuf, ModuleState>,
    reverse: &'a BTreeMap<PathBuf, Vec<ConsumerLink>>,
}

/// Run-level diagnostics gathered before the graph walk, folded into the result.
struct RunDiagnostics {
    warnings: Vec<String>,
    parse_failures: usize,
    unresolved_imports: usize,
    skipped_inputs: usize,
}

pub(super) struct ResolutionCache<'a> {
    resolver: &'a Resolver,
    entries: BTreeMap<(PathBuf, String), Resolution>,
}

impl<'a> ResolutionCache<'a> {
    fn new(resolver: &'a Resolver) -> Self {
        Self {
            resolver,
            entries: BTreeMap::new(),
        }
    }

    pub(super) fn resolve(&mut self, importer: &Path, specifier: &str) -> Resolution {
        let key = (importer.to_path_buf(), specifier.to_string());
        if let Some(resolution) = self.entries.get(&key) {
            return resolution.clone();
        }

        let resolution = self.resolver.resolve(importer, specifier);
        self.entries.insert(key, resolution.clone());
        resolution
    }

    pub(super) fn is_internal_specifier(&self, importer: &Path, specifier: &str) -> bool {
        self.resolver.is_internal_specifier(importer, specifier)
    }
}

pub fn run(cli: &Cli, context: &RepoContext) -> Result<AnalysisResult> {
    let resolver = Resolver::new(context)?;
    let mut resolution_cache = ResolutionCache::new(&resolver);
    let (modules, mut warnings, parse_failures) = load_modules(context);
    warnings.splice(0..0, context.warnings.iter().cloned());
    let module_states = build_module_states(&modules);
    let reverse = build_reverse_links(&modules, &module_states, &mut resolution_cache);
    let unresolved = unresolved_import_diagnostics(
        &modules,
        &mut resolution_cache,
        &context.ignore_unresolved,
        &context.repo_root,
        cli.explain_unresolved,
    );
    warnings.extend(unresolved.warnings);
    let analysis_data = AnalysisData {
        context,
        modules: &modules,
        module_states: &module_states,
        reverse: &reverse,
    };

    match &cli.command {
        // Handled in main before analysis ever runs.
        Command::Completions { .. } => bail!("completions does not run analysis"),
        Command::Export { file, export_name } => {
            let file = normalize_input_path(&context.repo_root, file)?;
            if !modules.contains_key(&file) {
                bail!(
                    "source file not found in repository index: {}",
                    file.display()
                );
            }
            validate_export_name(
                &file,
                export_name,
                &modules,
                &module_states,
                &context.repo_root,
                &mut warnings,
            )?;
            let exports =
                expanded_exports_for_root(&file, std::slice::from_ref(export_name), &module_states);

            analyze_from_roots(
                AnalysisMode::Export,
                AnalysisTarget::Export {
                    file: file.clone(),
                    export_name: export_name.clone(),
                },
                &analysis_data,
                RunDiagnostics {
                    warnings,
                    parse_failures,
                    unresolved_imports: unresolved.count,
                    skipped_inputs: 0,
                },
                vec![(file, exports)],
            )
        }
        Command::File { file } => {
            let file = normalize_input_path(&context.repo_root, file)?;
            if !modules.contains_key(&file) {
                bail!(
                    "source file not found in repository index: {}",
                    file.display()
                );
            }

            let exports = file_root_exports(&file, &module_states);

            analyze_from_roots(
                AnalysisMode::File,
                AnalysisTarget::File { file: file.clone() },
                &analysis_data,
                RunDiagnostics {
                    warnings,
                    parse_failures,
                    unresolved_imports: unresolved.count,
                    skipped_inputs: 0,
                },
                vec![(file, exports)],
            )
        }
        Command::Files { files } => {
            let mut roots = Vec::new();
            let mut normalized = Vec::new();
            let mut skipped_inputs = 0;
            // `files` mode is the entry point hook managers (lint-staged, Husky,
            // Lefthook) pipe changed paths into. Those batches can include
            // deleted/renamed files and non-source paths, so skip-and-warn on
            // anything we can't analyze rather than failing the whole run.
            let mut seen = BTreeSet::new();
            for file in files {
                let input = file.display().to_string();
                let Ok(file) = normalize_input_path(&context.repo_root, file) else {
                    skipped_inputs += 1;
                    warnings.push(format!("skipped input {input}: not found on disk"));
                    continue;
                };
                if !seen.insert(file.clone()) {
                    continue;
                }
                if !modules.contains_key(&file) {
                    skipped_inputs += 1;
                    warnings.push(format!(
                        "skipped input {}: not a recognized source file",
                        relative_label(&context.repo_root, &file)
                    ));
                    continue;
                }
                let exports = file_root_exports(&file, &module_states);
                roots.push((file.clone(), exports));
                normalized.push(file);
            }

            if normalized.is_empty() {
                warnings.push(format!(
                    "no recognized source files among {} input path{}",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ));
            }

            analyze_from_roots(
                AnalysisMode::Files,
                AnalysisTarget::Files { files: normalized },
                &analysis_data,
                RunDiagnostics {
                    warnings,
                    parse_failures,
                    unresolved_imports: unresolved.count,
                    skipped_inputs,
                },
                roots,
            )
        }
    }
}

/// Reject an export name the target file provably does not expose. When the
/// file's exports are not statically enumerable (star re-exports, whole-module
/// CommonJS assignment, or no parsed exports at all), proceed with a warning
/// instead of failing the run.
fn validate_export_name(
    file: &Path,
    export_name: &str,
    modules: &BTreeMap<PathBuf, ModuleFacts>,
    module_states: &BTreeMap<PathBuf, ModuleState>,
    repo_root: &Path,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let known = module_states.get(file).map(|state| &state.public_exports);
    if known.is_some_and(|exports| exports.contains(export_name)) {
        return Ok(());
    }

    let module = modules.get(file);
    let has_star_reexport = module.is_some_and(|module| {
        module
            .reexports
            .iter()
            .any(|reexport| matches!(reexport.imported, ReexportTarget::All))
    });
    let has_opaque_commonjs = module.is_some_and(|module| {
        module
            .exports
            .iter()
            .any(|export| export.kind == ExportKind::CommonJs && export.local.is_none())
    });
    let enumerable = known.is_some_and(|exports| !exports.is_empty());

    if has_star_reexport || has_opaque_commonjs || !enumerable {
        warnings.push(format!(
            "export '{export_name}' is not a statically-known export of {}; \
             continuing because the file's exports are not fully enumerable",
            relative_label(repo_root, file)
        ));
        return Ok(());
    }

    let available = known
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "export '{export_name}' not found in {}; available exports: {available}",
        relative_label(repo_root, file)
    );
}

fn file_root_exports(
    file: &Path,
    module_states: &BTreeMap<PathBuf, ModuleState>,
) -> BTreeSet<String> {
    module_states
        .get(file)
        .map(|state| {
            if state.public_exports.is_empty() {
                BTreeSet::from([String::from("*file*")])
            } else {
                state.public_exports.clone()
            }
        })
        .unwrap_or_else(|| BTreeSet::from([String::from("*file*")]))
}

fn load_modules(context: &RepoContext) -> (BTreeMap<PathBuf, ModuleFacts>, Vec<String>, usize) {
    let mut modules = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut parse_failures = 0;
    for file in &context.source_files {
        match parse_module(file) {
            Ok(facts) => {
                let relative = file.strip_prefix(&context.repo_root).unwrap_or(file);
                warnings.extend(
                    facts
                        .warnings
                        .iter()
                        .map(|warning| format!("{}: {warning}", relative.display())),
                );
                modules.insert(file.clone(), facts);
            }
            Err(error) => {
                parse_failures += 1;
                warnings.push(format!(
                    "skipped {}: {}",
                    file.strip_prefix(&context.repo_root)
                        .unwrap_or(file)
                        .display(),
                    error
                ));
            }
        }
    }
    (modules, warnings, parse_failures)
}

fn build_module_states(modules: &BTreeMap<PathBuf, ModuleFacts>) -> BTreeMap<PathBuf, ModuleState> {
    let mut states = BTreeMap::new();

    for module in modules.values() {
        let mut state = ModuleState::new(module.file.clone());
        for export in &module.exports {
            state.add_export(export.exported.clone(), export.local.clone());
            if export.kind == ExportKind::Default {
                state.public_exports.insert("default".to_string());
            }
        }
        for reexport in &module.reexports {
            if reexport.exported != "*" {
                state.add_export(reexport.exported.clone(), None);
            }
        }
        states.insert(module.file.clone(), state);
    }

    states
}

fn analyze_from_roots(
    mode: AnalysisMode,
    target: AnalysisTarget,
    data: &AnalysisData<'_>,
    diagnostics: RunDiagnostics,
    roots: Vec<(PathBuf, BTreeSet<String>)>,
) -> Result<AnalysisResult> {
    let RunDiagnostics {
        mut warnings,
        parse_failures,
        unresolved_imports,
        skipped_inputs,
    } = diagnostics;
    let ambiguous_edges = data
        .modules
        .values()
        .map(|module| {
            let star_reexport_count = module
                .reexports
                .iter()
                .filter(|fact| matches!(fact.imported, ReexportTarget::All))
                .count();
            module
                .reexports
                .iter()
                .filter(|fact| match fact.imported {
                    ReexportTarget::All => star_reexport_count > 1,
                    _ => fact.is_ambiguous,
                })
                .count()
        })
        .sum::<usize>();
    if parse_failures > 0 {
        warnings.insert(
            0,
            format!(
                "{} source file{} could not be parsed and {} skipped",
                parse_failures,
                if parse_failures == 1 { "" } else { "s" },
                if parse_failures == 1 { "was" } else { "were" }
            ),
        );
    }
    let workspaces = collect_workspaces(data.context);

    let (states, reasons) = run_bfs(&roots, data.modules, data.module_states, data.reverse);

    // For a multi-file run, also compute each file's blast radius on its own so
    // the report can break impact down per file.
    let root_impacts = if roots.len() > 1 {
        compute_root_impacts(
            &roots,
            data.modules,
            data.module_states,
            data.reverse,
            &workspaces,
            &data.context.repo_root,
        )
    } else {
        Vec::new()
    };

    let metadata = ResultMetadata {
        mode,
        target,
        warnings,
        parse_failures,
        unresolved_imports,
        ambiguous_edges,
        skipped_inputs,
        workspaces,
        root_impacts,
    };

    let result = build_result(data.context, data.module_states, states, reasons, metadata);

    Ok(result)
}

fn expanded_exports_for_root(
    file: &Path,
    names: &[String],
    module_states: &BTreeMap<PathBuf, ModuleState>,
) -> BTreeSet<String> {
    let mut expanded: BTreeSet<String> = names.iter().cloned().collect();
    let Some(module_state) = module_states.get(file) else {
        return expanded;
    };

    let mut locals = BTreeSet::new();
    for name in names {
        if let Some(related) = module_state.export_to_locals.get(name) {
            locals.extend(related.iter().cloned());
        }
    }

    for local in locals {
        if let Some(exports) = module_state.local_to_exports.get(&local) {
            expanded.extend(exports.iter().cloned());
        }
    }

    expanded
}

fn normalize_input_path(repo_root: &Path, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    joined
        .canonicalize()
        .with_context(|| format!("failed to resolve input path {}", joined.display()))
}

/// Node ids embed absolute paths; normalize separators so JSON output (edge
/// `from`/`to`, node ids) is stable across platforms. Consumers that rebuild
/// ids from paths (e.g. the cascade renderer) must normalize the same way.
pub(super) fn file_id(path: &Path) -> String {
    crate::graph::normalize_separators(format!("file:{}", path.display()))
}

pub(super) fn export_id(path: &Path, export: &str) -> String {
    crate::graph::normalize_separators(format!("export:{}#{export}", path.display()))
}

/// Render a path relative to the repo root, always with `/` separators —
/// `Path::display()` yields `\` on Windows, which would break the `/`-based
/// package and directory grouping downstream.
pub(super) fn relative_label(repo_root: &Path, path: &Path) -> String {
    crate::graph::normalize_separators(
        path.strip_prefix(repo_root)
            .unwrap_or(path)
            .display()
            .to_string(),
    )
}
