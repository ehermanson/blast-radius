use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::fs::RepoContext;
use crate::graph::{
    AnalysisMode, AnalysisResult, AnalysisTarget, GraphEdge, GraphNode, ModuleState, NodeKind,
    RootImpact, Summary, Workspace,
};

use super::walk::{AffectedState, ImpactReason};
use super::{export_id, file_id, relative_label};

pub(super) struct ResultMetadata {
    pub(super) mode: AnalysisMode,
    pub(super) target: AnalysisTarget,
    pub(super) warnings: Vec<String>,
    pub(super) parse_failures: usize,
    pub(super) unresolved_imports: usize,
    pub(super) ambiguous_edges: usize,
    pub(super) skipped_inputs: usize,
    pub(super) workspaces: Vec<Workspace>,
    pub(super) root_impacts: Vec<RootImpact>,
}

pub(super) fn build_result(
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
            skipped_inputs: metadata.skipped_inputs,
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
pub(super) fn collect_workspaces(context: &RepoContext) -> Vec<Workspace> {
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
