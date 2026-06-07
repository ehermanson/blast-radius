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
use diagnostics::count_unresolved_imports;

struct AnalysisData<'a> {
    context: &'a RepoContext,
    modules: &'a BTreeMap<PathBuf, ModuleFacts>,
    module_states: &'a BTreeMap<PathBuf, ModuleState>,
    reverse: &'a BTreeMap<PathBuf, Vec<ConsumerLink>>,
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
    warnings.extend(resolver.warnings());
    let module_states = build_module_states(&modules);
    let reverse = build_reverse_links(&modules, &module_states, &mut resolution_cache);
    let unresolved_imports = count_unresolved_imports(&modules, &mut resolution_cache);
    let analysis_data = AnalysisData {
        context,
        modules: &modules,
        module_states: &module_states,
        reverse: &reverse,
    };

    match &cli.command {
        Command::Export { file, export_name } => {
            let file = normalize_input_path(&context.repo_root, file)?;
            if !modules.contains_key(&file) {
                bail!(
                    "source file not found in repository index: {}",
                    file.display()
                );
            }
            let exports =
                expanded_exports_for_root(&file, std::slice::from_ref(export_name), &module_states);

            analyze_from_roots(
                AnalysisMode::Export,
                AnalysisTarget::Export {
                    file: file.clone(),
                    export_name: export_name.clone(),
                },
                &analysis_data,
                warnings,
                parse_failures,
                unresolved_imports,
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
                warnings,
                parse_failures,
                unresolved_imports,
                vec![(file, exports)],
            )
        }
        Command::Files { files } => {
            let mut roots = Vec::new();
            let mut normalized = Vec::new();
            for file in files {
                let file = normalize_input_path(&context.repo_root, file)?;
                if !modules.contains_key(&file) {
                    bail!(
                        "source file not found in repository index: {}",
                        file.display()
                    );
                }
                let exports = file_root_exports(&file, &module_states);
                roots.push((file.clone(), exports));
                normalized.push(file);
            }

            analyze_from_roots(
                AnalysisMode::Files,
                AnalysisTarget::Files { files: normalized },
                &analysis_data,
                warnings,
                parse_failures,
                unresolved_imports,
                roots,
            )
        }
    }
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
    mut warnings: Vec<String>,
    parse_failures: usize,
    unresolved_imports: usize,
    roots: Vec<(PathBuf, BTreeSet<String>)>,
) -> Result<AnalysisResult> {
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

pub(super) fn file_id(path: &Path) -> String {
    format!("file:{}", path.display())
}

pub(super) fn export_id(path: &Path, export: &str) -> String {
    format!("export:{}#{export}", path.display())
}

pub(super) fn relative_label(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}
