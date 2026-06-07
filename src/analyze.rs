use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::{Cli, Command};
use crate::fs::RepoContext;
use crate::graph::{
    AnalysisMode, AnalysisResult, AnalysisTarget, EdgeKind, GraphEdge, GraphNode, ModuleState,
    NodeKind, RootImpact, RootImpactFile, Summary, Workspace, package_key,
};
use crate::parse::{
    ExportKind, ImportKind, ImportTarget, ModuleFacts, ReexportTarget, parse_module,
};
use crate::resolve::{Resolution, Resolver};

#[derive(Debug, Clone)]
struct ConsumerLink {
    consumer_file: PathBuf,
    relation: ConsumerRelation,
}

#[derive(Debug, Clone)]
enum ConsumerRelation {
    Import {
        imported: ImportTarget,
        local: String,
        kind: EdgeKind,
    },
    LocalExport {
        imported: ImportTarget,
        local: String,
        exported: String,
    },
    Reexport {
        imported: ReexportTarget,
        exported: String,
        kind: EdgeKind,
        is_ambiguous: bool,
    },
}

#[derive(Debug, Clone)]
struct ImpactReason {
    parent_id: String,
    child_id: String,
    kind: EdgeKind,
    is_ambiguous: bool,
}

#[derive(Debug, Clone)]
struct AffectedState {
    depth: usize,
    affected_exports: BTreeSet<String>,
    file_affected: bool,
}

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

struct ResolutionCache<'a> {
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

    fn resolve(&mut self, importer: &Path, specifier: &str) -> Resolution {
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

            let exports = module_states
                .get(&file)
                .map(|state| {
                    if state.public_exports.is_empty() {
                        BTreeSet::from([String::from("*file*")])
                    } else {
                        state.public_exports.clone()
                    }
                })
                .unwrap_or_default();

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
                let exports = module_states
                    .get(&file)
                    .map(|state| {
                        if state.public_exports.is_empty() {
                            BTreeSet::from([String::from("*file*")])
                        } else {
                            state.public_exports.clone()
                        }
                    })
                    .unwrap_or_else(|| BTreeSet::from([String::from("*file*")]));
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

fn build_reverse_links(
    modules: &BTreeMap<PathBuf, ModuleFacts>,
    module_states: &BTreeMap<PathBuf, ModuleState>,
    resolution_cache: &mut ResolutionCache<'_>,
) -> BTreeMap<PathBuf, Vec<ConsumerLink>> {
    let mut reverse: BTreeMap<PathBuf, Vec<ConsumerLink>> = BTreeMap::new();

    for module in modules.values() {
        let star_reexport_count = module
            .reexports
            .iter()
            .filter(|reexport| matches!(reexport.imported, ReexportTarget::All))
            .count();

        for import in &module.imports {
            let Resolution::Resolved(target) =
                resolution_cache.resolve(&module.file, &import.source)
            else {
                continue;
            };

            reverse
                .entry(target.clone())
                .or_default()
                .push(ConsumerLink {
                    consumer_file: module.file.clone(),
                    relation: ConsumerRelation::Import {
                        imported: import.imported.clone(),
                        local: import.local.clone(),
                        kind: match (&import.imported, import.kind) {
                            (ImportTarget::Default, ImportKind::CommonJs) => {
                                EdgeKind::RequiresModule
                            }
                            (ImportTarget::Name(_), ImportKind::CommonJs) => {
                                EdgeKind::RequiresModule
                            }
                            (ImportTarget::Namespace, ImportKind::CommonJs) => {
                                EdgeKind::RequiresModule
                            }
                            (ImportTarget::Default, _) => EdgeKind::ImportsDefault,
                            (ImportTarget::Name(_), _) => EdgeKind::ImportsNamed,
                            (ImportTarget::Namespace, _) => EdgeKind::ImportsNamespace,
                        },
                    },
                });

            if let Some(consumer_state) = module_states.get(&module.file)
                && let Some(exported_names) = consumer_state.local_to_exports.get(&import.local)
            {
                for exported in exported_names {
                    reverse
                        .entry(target.clone())
                        .or_default()
                        .push(ConsumerLink {
                            consumer_file: module.file.clone(),
                            relation: ConsumerRelation::LocalExport {
                                imported: import.imported.clone(),
                                local: import.local.clone(),
                                exported: exported.clone(),
                            },
                        });
                }
            }
        }

        for reexport in &module.reexports {
            let Resolution::Resolved(target) =
                resolution_cache.resolve(&module.file, &reexport.source)
            else {
                continue;
            };

            reverse.entry(target).or_default().push(ConsumerLink {
                consumer_file: module.file.clone(),
                relation: ConsumerRelation::Reexport {
                    imported: reexport.imported.clone(),
                    exported: reexport.exported.clone(),
                    kind: match reexport.imported {
                        ReexportTarget::All => EdgeKind::ReexportsStar,
                        _ => EdgeKind::ReexportsNamed,
                    },
                    is_ambiguous: match reexport.imported {
                        ReexportTarget::All => star_reexport_count > 1,
                        _ => reexport.is_ambiguous,
                    },
                },
            });
        }
    }

    reverse
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

/// Walk the reverse-dependency graph from a set of roots, returning the affected
/// files (with depth) and the edges that explain each impact.
fn run_bfs(
    roots: &[(PathBuf, BTreeSet<String>)],
    modules: &BTreeMap<PathBuf, ModuleFacts>,
    module_states: &BTreeMap<PathBuf, ModuleState>,
    reverse: &BTreeMap<PathBuf, Vec<ConsumerLink>>,
) -> (BTreeMap<PathBuf, AffectedState>, Vec<ImpactReason>) {
    let mut states: BTreeMap<PathBuf, AffectedState> = BTreeMap::new();
    let mut queue = VecDeque::new();
    let mut reasons: Vec<ImpactReason> = Vec::new();

    for (file, exports) in roots {
        let entry = states.entry(file.clone()).or_insert(AffectedState {
            depth: 0,
            affected_exports: BTreeSet::new(),
            file_affected: true,
        });
        entry.affected_exports.extend(exports.clone());
        entry.file_affected = true;
        queue.push_back(file.clone());
    }

    while let Some(current_file) = queue.pop_front() {
        let current_state = states
            .get(&current_file)
            .cloned()
            .expect("queued file must exist in state");
        let current_exports = current_state.affected_exports.clone();

        let Some(consumers) = reverse.get(&current_file) else {
            continue;
        };

        for link in consumers {
            let Some(consumer_module) = modules.get(&link.consumer_file) else {
                continue;
            };
            let consumer_public_exports = module_states
                .get(&link.consumer_file)
                .map(|state| state.public_exports.clone())
                .unwrap_or_default();

            let mut newly_added_exports = BTreeSet::new();
            let mut file_affected = false;
            let mut edge_kind = None;
            let mut child_id = file_id(&link.consumer_file);
            let mut ambiguous = false;

            match &link.relation {
                ConsumerRelation::Import {
                    imported,
                    local,
                    kind,
                } => {
                    if import_matches(imported, &current_exports, consumer_module, local) {
                        file_affected = true;
                        edge_kind = Some(if is_jsx_usage(consumer_module, local) {
                            EdgeKind::UsesJsxComponent
                        } else {
                            *kind
                        });
                        if !consumer_public_exports.is_empty() {
                            newly_added_exports.extend(consumer_public_exports.clone());
                        }
                    }
                }
                ConsumerRelation::LocalExport {
                    imported,
                    local,
                    exported,
                } => {
                    if import_target_matches(imported, &current_exports, consumer_module, local) {
                        file_affected = true;
                        newly_added_exports.insert(exported.clone());
                        child_id = export_id(&link.consumer_file, exported);
                        edge_kind = Some(EdgeKind::CommonJsExport);
                    }
                }
                ConsumerRelation::Reexport {
                    imported,
                    exported,
                    kind,
                    is_ambiguous,
                } => {
                    if reexport_matches(imported, &current_exports) {
                        file_affected = true;
                        edge_kind = Some(*kind);
                        ambiguous = *is_ambiguous;
                        if matches!(imported, ReexportTarget::All) {
                            newly_added_exports.extend(current_exports.clone());
                        } else if exported != "*" {
                            newly_added_exports.insert(exported.clone());
                            child_id = export_id(&link.consumer_file, exported);
                        }
                    }
                }
            }

            if !file_affected {
                continue;
            }

            let parent_id = if current_state.depth == 0 && current_exports.len() == 1 {
                let export = current_exports.iter().next().cloned().unwrap_or_default();
                if export == "*file*" {
                    file_id(&current_file)
                } else {
                    export_id(&current_file, &export)
                }
            } else {
                file_id(&current_file)
            };

            let next_depth = current_state.depth + 1;
            let entry = states
                .entry(link.consumer_file.clone())
                .or_insert(AffectedState {
                    depth: next_depth,
                    affected_exports: BTreeSet::new(),
                    file_affected: false,
                });

            let old_len = entry.affected_exports.len();
            if !newly_added_exports.is_empty() {
                entry.affected_exports.extend(newly_added_exports);
            }
            let exports_changed = entry.affected_exports.len() != old_len;
            let depth_changed = if next_depth < entry.depth {
                entry.depth = next_depth;
                true
            } else {
                false
            };
            let file_changed = if !entry.file_affected {
                entry.file_affected = true;
                true
            } else {
                false
            };

            if exports_changed || depth_changed || file_changed {
                queue.push_back(link.consumer_file.clone());
            }

            if let Some(kind) = edge_kind {
                reasons.push(ImpactReason {
                    parent_id,
                    child_id,
                    kind,
                    is_ambiguous: ambiguous,
                });
            }
        }
    }

    (states, reasons)
}

/// Compute each input file's individual blast radius by running the walk from
/// that single file. Sorted by reach, widest first.
fn compute_root_impacts(
    roots: &[(PathBuf, BTreeSet<String>)],
    modules: &BTreeMap<PathBuf, ModuleFacts>,
    module_states: &BTreeMap<PathBuf, ModuleState>,
    reverse: &BTreeMap<PathBuf, Vec<ConsumerLink>>,
    workspaces: &[Workspace],
    repo_root: &Path,
) -> Vec<RootImpact> {
    let mut impacts: Vec<RootImpact> = roots
        .iter()
        .map(|root| {
            let (states, _) = run_bfs(std::slice::from_ref(root), modules, module_states, reverse);

            let mut direct = 0;
            let mut indirect = 0;
            let mut max_depth = 0;
            let mut packages = BTreeSet::new();
            let mut files = Vec::new();
            for (file, state) in &states {
                if state.depth < 1 {
                    continue;
                }
                if state.depth == 1 {
                    direct += 1;
                } else {
                    indirect += 1;
                }
                max_depth = max_depth.max(state.depth);
                packages.insert(package_key(&relative_label(repo_root, file), workspaces));

                // An endpoint has no affected file depending on it in turn.
                let endpoint = match reverse.get(file) {
                    None => true,
                    Some(links) => !links.iter().any(|link| {
                        states
                            .get(&link.consumer_file)
                            .is_some_and(|s| s.depth >= 1)
                    }),
                };
                files.push(RootImpactFile {
                    path: relative_label(repo_root, file),
                    endpoint,
                });
            }
            files.sort_by(|a, b| a.path.cmp(&b.path));

            RootImpact {
                file: relative_label(repo_root, &root.0),
                affected: direct + indirect,
                direct,
                indirect,
                max_depth,
                packages: packages.len(),
                files,
            }
        })
        .collect();

    impacts.sort_by(|a, b| b.affected.cmp(&a.affected).then(a.file.cmp(&b.file)));
    impacts
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

fn import_matches(
    imported: &ImportTarget,
    current_exports: &BTreeSet<String>,
    module: &ModuleFacts,
    local: &str,
) -> bool {
    import_target_matches(imported, current_exports, module, local)
        && (module.used_locals.contains(local)
            || module
                .namespace_member_usage
                .get(local)
                .map(|members| {
                    members
                        .iter()
                        .any(|member| current_exports.contains(member))
                })
                .unwrap_or(false))
}

fn import_target_matches(
    imported: &ImportTarget,
    current_exports: &BTreeSet<String>,
    module: &ModuleFacts,
    local: &str,
) -> bool {
    let _ = module;
    match imported {
        ImportTarget::Name(name) => current_exports.contains(name),
        ImportTarget::Default => {
            current_exports.contains("default")
                || current_exports.contains("*file*")
                || current_exports.contains(local)
        }
        ImportTarget::Namespace => module
            .namespace_member_usage
            .get(local)
            .map(|members| {
                members
                    .iter()
                    .any(|member| current_exports.contains(member))
            })
            .unwrap_or_else(|| module.used_locals.contains(local) && !current_exports.is_empty()),
    }
}

fn reexport_matches(imported: &ReexportTarget, current_exports: &BTreeSet<String>) -> bool {
    match imported {
        ReexportTarget::Name(name) => current_exports.contains(name),
        ReexportTarget::Default => current_exports.contains("default"),
        ReexportTarget::Namespace => !current_exports.is_empty(),
        ReexportTarget::All => !current_exports.is_empty(),
    }
}

fn is_jsx_usage(module: &ModuleFacts, local: &str) -> bool {
    module.namespace_member_usage.contains_key(local) || module.used_locals.contains(local)
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

fn file_id(path: &Path) -> String {
    format!("file:{}", path.display())
}

fn export_id(path: &Path, export: &str) -> String {
    format!("export:{}#{export}", path.display())
}

fn relative_label(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}
