use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::{Cli, Command};
use crate::fs::RepoContext;
use crate::graph::{
    AnalysisMode, AnalysisResult, AnalysisTarget, GraphEdge, GraphNode, ModuleState, NodeKind,
    RootImpact, Summary, Workspace,
};
use crate::parse::{ExportKind, ModuleFacts, ReexportTarget, parse_module};
use crate::resolve::{Resolution, Resolver};

mod walk;
use walk::{
    AffectedState, ConsumerLink, ImpactReason, build_reverse_links, compute_root_impacts, run_bfs,
};

struct ResultMetadata {
    mode: AnalysisMode,
    target: AnalysisTarget,
    warnings: Vec<String>,
    parse_failures: usize,
    unresolved_imports: usize,
    ambiguous_edges: usize,
    workspaces: Vec<Workspace>,
    root_impacts: Vec<RootImpact>,
}

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

    fn is_internal_specifier(&self, importer: &Path, specifier: &str) -> bool {
        self.resolver.is_internal_specifier(importer, specifier)
    }
}

pub fn run(cli: &Cli, context: &RepoContext) -> Result<AnalysisResult> {
    let resolver = Resolver::new(context)?;
    let mut resolution_cache = ResolutionCache::new(&resolver);
    let (modules, parse_warnings, parse_failures) = load_modules(context);
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
                parse_warnings,
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
                parse_warnings,
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
                parse_warnings,
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

fn build_result(
    context: &RepoContext,
    module_states: &BTreeMap<PathBuf, ModuleState>,
    states: BTreeMap<PathBuf, AffectedState>,
    reasons: Vec<ImpactReason>,
    metadata: ResultMetadata,
) -> AnalysisResult {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen_nodes = BTreeSet::new();
    let mut seen_edges = BTreeSet::new();

    for (file, state) in &states {
        let file_id = file_id(file);
        if seen_nodes.insert(file_id.clone()) {
            nodes.push(GraphNode {
                id: file_id.clone(),
                label: relative_label(&context.repo_root, file),
                file: file.clone(),
                symbol: None,
                kind: NodeKind::File,
                depth: state.depth,
            });
        }

        if let Some(module_state) = module_states.get(file) {
            for export in state
                .affected_exports
                .iter()
                .filter(|name| module_state.public_exports.contains(*name))
            {
                let export_id = export_id(file, export);
                if seen_nodes.insert(export_id.clone()) {
                    nodes.push(GraphNode {
                        id: export_id.clone(),
                        label: format!("{}#{}", relative_label(&context.repo_root, file), export),
                        file: file.clone(),
                        symbol: Some(export.clone()),
                        kind: NodeKind::Export,
                        depth: state.depth,
                    });
                }
            }
        }
    }

    for reason in reasons {
        let key = format!(
            "{}->{}:{:?}:{}",
            reason.parent_id, reason.child_id, reason.kind, reason.is_ambiguous
        );
        if seen_edges.insert(key) {
            edges.push(GraphEdge {
                from: reason.parent_id,
                to: reason.child_id,
                kind: reason.kind,
                is_ambiguous: reason.is_ambiguous,
            });
        }
    }

    let total_affected_files = states.len();
    let directly_affected_files = states.values().filter(|state| state.depth == 1).count();
    let transitively_affected_files = states.values().filter(|state| state.depth > 1).count();

    AnalysisResult {
        mode: metadata.mode,
        target: metadata.target,
        repo_root: context.repo_root.clone(),
        source_file_count: context.source_files.len(),
        summary: Summary {
            directly_affected_files,
            transitively_affected_files,
            total_affected_files,
            unresolved_imports: metadata.unresolved_imports,
            ambiguous_edges: metadata.ambiguous_edges,
            parse_failures: metadata.parse_failures,
        },
        workspaces: metadata.workspaces,
        roots: metadata.root_impacts,
        nodes,
        edges,
        warnings: metadata.warnings,
    }
}

/// Build the list of workspace packages from the discovered `package.json`
/// files. Sorted by descending root length so longest-prefix matching picks the
/// most specific package for a given file.
fn collect_workspaces(context: &RepoContext) -> Vec<Workspace> {
    let mut workspaces = Vec::new();
    for package_json in &context.package_jsons {
        let Some(parent) = package_json.parent() else {
            continue;
        };
        let root = relative_label(&context.repo_root, parent);
        let name = std::fs::read_to_string(package_json)
            .ok()
            .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
            .and_then(|value| {
                value
                    .get("name")
                    .and_then(|name| name.as_str())
                    .map(str::to_string)
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                if root.is_empty() {
                    ".".to_string()
                } else {
                    root.clone()
                }
            });
        workspaces.push(Workspace { name, root });
    }

    workspaces.sort_by(|a, b| b.root.len().cmp(&a.root.len()).then(a.root.cmp(&b.root)));
    workspaces
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

fn count_unresolved_imports(
    modules: &BTreeMap<PathBuf, ModuleFacts>,
    resolution_cache: &mut ResolutionCache<'_>,
) -> usize {
    let mut count = 0;

    for module in modules.values() {
        for import in &module.imports {
            if !should_count_unresolved_import(import) {
                continue;
            }
            if !resolution_cache.is_internal_specifier(&module.file, &import.source) {
                continue;
            }
            if matches!(
                resolution_cache.resolve(&module.file, &import.source),
                Resolution::Unresolved
            ) {
                count += 1;
            }
        }
    }

    count
}

fn should_count_unresolved_import(import: &crate::parse::ImportFact) -> bool {
    if import.type_only {
        return false;
    }

    let source = import.source.as_str();
    if source.contains(".velite") {
        return false;
    }
    if source.contains("/+types/") || source.starts_with("./+types/") {
        return false;
    }
    if source.ends_with("package.json") {
        return false;
    }
    if source.ends_with(".svg") {
        return false;
    }
    if source.contains("styled-system/recipes")
        || source.contains("styled-system/patterns")
        || source.contains("styled-system/css")
    {
        return false;
    }
    if source.contains("/dist/esm/") || source.contains("/dist/cjs/") {
        return false;
    }

    true
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
