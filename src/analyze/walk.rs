use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::graph::{EdgeKind, ModuleState, RootImpact, RootImpactFile, Workspace, package_key};
use crate::parse::{ImportKind, ImportTarget, ModuleFacts, ReexportTarget};
use crate::resolve::Resolution;

use super::{ResolutionCache, export_id, file_id, relative_label};

#[derive(Debug, Clone)]
pub(super) struct ConsumerLink {
    pub(super) consumer_file: PathBuf,
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
pub(super) struct ImpactReason {
    pub(super) parent_id: String,
    pub(super) child_id: String,
    pub(super) kind: EdgeKind,
    pub(super) is_ambiguous: bool,
}

#[derive(Debug, Clone)]
pub(super) struct AffectedState {
    pub(super) depth: usize,
    pub(super) affected_exports: BTreeSet<String>,
    file_affected: bool,
}
pub(super) fn build_reverse_links(
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
                        kind: import_edge_kind(&import.imported, import.kind),
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
/// The edge kind for an import, by what it binds and how it's loaded. Shared by
/// the reverse-link builder and the whole-repo `graph` dump.
pub(super) fn import_edge_kind(imported: &ImportTarget, kind: ImportKind) -> EdgeKind {
    match (imported, kind) {
        (_, ImportKind::Mock) => EdgeKind::MocksModule,
        (_, ImportKind::Dynamic) => EdgeKind::ImportsDynamic,
        (_, ImportKind::CommonJs) => EdgeKind::RequiresModule,
        (ImportTarget::Default, _) => EdgeKind::ImportsDefault,
        (ImportTarget::Name(_), _) => EdgeKind::ImportsNamed,
        (ImportTarget::Namespace, _) => EdgeKind::ImportsNamespace,
        // A side-effect import depends on the whole module, like a namespace.
        (ImportTarget::SideEffect, _) => EdgeKind::ImportsNamespace,
    }
}

/// Walk the reverse-dependency graph from a set of roots, returning the affected
/// files (with depth) and the edges that explain each impact.
pub(super) fn run_bfs(
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
                        match imported {
                            ReexportTarget::All => {
                                newly_added_exports.extend(current_exports.clone());
                            }
                            // `export * as ns`: qualify the affected upstream
                            // exports under the alias (`ns.Button`) so member-level
                            // consumers stay precise. A non-enumerable upstream
                            // (`*file*`) collapses to the bare alias: the whole
                            // object is affected.
                            ReexportTarget::Namespace if exported != "*" => {
                                if current_exports.contains("*file*") {
                                    newly_added_exports.insert(exported.clone());
                                } else {
                                    for export in &current_exports {
                                        newly_added_exports.insert(format!("{exported}.{export}"));
                                    }
                                }
                                child_id = export_id(&link.consumer_file, exported);
                            }
                            // A named re-export of a namespace object
                            // (`export { ns as kit } from './barrel'`) carries the
                            // qualified members across under the new alias.
                            ReexportTarget::Name(name) if exported != "*" => {
                                if current_exports.contains(name)
                                    || current_exports.contains("*file*")
                                {
                                    newly_added_exports.insert(exported.clone());
                                }
                                for member in member_entries(&current_exports, name) {
                                    newly_added_exports.insert(format!("{exported}.{member}"));
                                }
                                child_id = export_id(&link.consumer_file, exported);
                            }
                            _ if exported != "*" => {
                                newly_added_exports.insert(exported.clone());
                                child_id = export_id(&link.consumer_file, exported);
                            }
                            _ => {}
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
pub(super) fn compute_root_impacts(
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
                    depth: state.depth,
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
fn import_matches(
    imported: &ImportTarget,
    current_exports: &BTreeSet<String>,
    module: &ModuleFacts,
    local: &str,
) -> bool {
    // A side-effect import binds nothing, so there is no local usage to gate
    // on: reaching the file is the impact.
    if matches!(imported, ImportTarget::SideEffect) {
        return true;
    }
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
    // `*file*` marks a file-level root whose exports are not statically
    // enumerable (e.g. a star-only barrel): the whole file changed, so any
    // import that resolves to it is affected.
    if current_exports.contains("*file*") {
        return true;
    }
    match imported {
        // The importer depends on the whole file, whatever changed in it.
        ImportTarget::SideEffect => true,
        ImportTarget::Name(name) => {
            if current_exports.contains(name) {
                return true;
            }
            // The name may be a namespace object whose affected members travel
            // as qualified entries (`ns.Button`). When the consumer's member
            // usage is known, match member-precisely; otherwise any affected
            // member means the object the consumer holds is affected.
            match module.namespace_member_usage.get(local) {
                Some(members) => members
                    .iter()
                    .any(|member| qualified_hit(current_exports, name, member)),
                None => !member_entries(current_exports, name).is_empty(),
            }
        }
        ImportTarget::Default => {
            current_exports.contains("default") || current_exports.contains(local)
        }
        ImportTarget::Namespace => module
            .namespace_member_usage
            .get(local)
            .map(|members| {
                members.iter().any(|member| {
                    current_exports.contains(member)
                        || !member_entries(current_exports, member).is_empty()
                })
            })
            .unwrap_or_else(|| module.used_locals.contains(local) && !current_exports.is_empty()),
    }
}

fn reexport_matches(imported: &ReexportTarget, current_exports: &BTreeSet<String>) -> bool {
    if current_exports.contains("*file*") {
        return true;
    }
    match imported {
        ReexportTarget::Name(name) => {
            current_exports.contains(name) || !member_entries(current_exports, name).is_empty()
        }
        ReexportTarget::Default => current_exports.contains("default"),
        ReexportTarget::Namespace => !current_exports.is_empty(),
        ReexportTarget::All => !current_exports.is_empty(),
    }
}

/// Affected-export entries qualified under `name` (`name.<member>`), returned
/// as the member part. `ns.Button` under `ns` yields `Button`; deeper chains
/// (`outer.ns.Button` under `outer`) yield `ns.Button`.
fn member_entries<'a>(current_exports: &'a BTreeSet<String>, name: &str) -> Vec<&'a str> {
    current_exports
        .iter()
        .filter_map(|entry| {
            entry
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('.'))
        })
        .collect()
}

/// Whether `name.member` is affected, either exactly or as a prefix of a
/// deeper qualified entry (`ns.Button` hits for member `Button`; `outer.ns.X`
/// hits for member `ns` under `outer`).
fn qualified_hit(current_exports: &BTreeSet<String>, name: &str, member: &str) -> bool {
    current_exports.iter().any(|entry| {
        entry
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|rest| rest.strip_prefix(member))
            .is_some_and(|tail| tail.is_empty() || tail.starts_with('.'))
    })
}

fn is_jsx_usage(module: &ModuleFacts, local: &str) -> bool {
    module.jsx_namespace_member_usage.contains_key(local) || module.jsx_locals.contains(local)
}
