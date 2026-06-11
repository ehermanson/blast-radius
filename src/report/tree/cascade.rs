use std::collections::BTreeSet;

use crate::graph::{AnalysisMode, AnalysisResult, AnalysisTarget, EdgeKind, GraphNode, NodeKind};

use crate::report::theme::Theme;

/// The detailed root -> direct -> cascade tree, shown only with `--verbose`.
pub(super) fn render_cascade(result: &AnalysisResult, theme: &Theme, lines: &mut Vec<String>) {
    let kind_counts = count_node_kinds(result);
    lines.push(String::new());
    lines.push(theme.rule("cascade · overview"));
    lines.push(format!(
        "{} {} files  {} exports",
        theme.key("nodes"),
        theme.number(kind_counts.files),
        theme.number(kind_counts.exports)
    ));

    if result.nodes.is_empty() {
        lines.push(format!(
            "{} {}",
            theme.muted("•"),
            theme.muted("No affected files found")
        ));
    } else if let Some(root_id) = preferred_root(result) {
        let direct_edges = visible_child_edges(&root_id, result);

        if let Some(root_node) = find_node(result, &root_id) {
            lines.push(format!(
                "{} {}",
                theme.root("root"),
                format_node(root_node, theme)
            ));
        }

        if direct_edges.is_empty() {
            lines.push(format!(
                "{} {}",
                theme.muted("•"),
                theme.muted("No downstream dependents found")
            ));
        } else {
            for edge in &direct_edges {
                lines.push(direct_child_line(edge, result, theme));
            }
        }

        let cascading_children: Vec<String> = direct_edges
            .iter()
            .filter(|edge| has_children(&edge.to, result))
            .map(|edge| edge.to.clone())
            .collect();

        lines.push(String::new());
        lines.push(theme.rule("cascade · paths"));

        if cascading_children.is_empty() {
            lines.push(format!(
                "{} {}",
                theme.muted("•"),
                theme.muted("No transitive paths beyond the direct dependents")
            ));
        } else {
            let mut walk = BranchWalk::default();
            for (index, child_id) in cascading_children.iter().enumerate() {
                let is_last_branch = index + 1 == cascading_children.len();
                walk.path.clear();
                walk.path.insert(root_id.clone());
                render_path_branch(
                    child_id,
                    result,
                    "",
                    is_last_branch,
                    &mut walk,
                    lines,
                    theme,
                );
            }
        }
    }
}

pub(super) fn is_leaf(node_id: &str, result: &AnalysisResult) -> bool {
    !result.edges.iter().any(|edge| edge.from == node_id)
}

pub(super) fn format_mode(mode: &AnalysisMode) -> &'static str {
    match mode {
        AnalysisMode::Export => "export",
        AnalysisMode::File => "file",
        AnalysisMode::Files => "files",
    }
}

/// Must mirror `analyze::file_id`: ids embed `/`-normalized absolute paths.
fn rebuilt_file_id(file: &std::path::Path) -> String {
    crate::graph::normalize_separators(format!("file:{}", file.display()))
}

fn preferred_root(result: &AnalysisResult) -> Option<String> {
    match &result.target {
        AnalysisTarget::Export { file, .. } => {
            let file_id = rebuilt_file_id(file);
            find_existing_node(result, &[&file_id])
        }
        AnalysisTarget::File { file } => {
            let file_id = rebuilt_file_id(file);
            find_existing_node(result, &[&file_id])
        }
        AnalysisTarget::Files { files } => {
            let preferred: Vec<String> = files.iter().map(|file| rebuilt_file_id(file)).collect();
            let preferred_refs: Vec<&str> = preferred.iter().map(String::as_str).collect();
            find_existing_node(result, &preferred_refs)
        }
    }
}

fn find_existing_node(result: &AnalysisResult, ids: &[&str]) -> Option<String> {
    ids.iter()
        .find(|id| result.nodes.iter().any(|node| node.id == **id))
        .map(|id| (*id).to_string())
}

fn find_node<'a>(result: &'a AnalysisResult, id: &str) -> Option<&'a GraphNode> {
    result.nodes.iter().find(|node| node.id == id)
}

fn has_children(node_id: &str, result: &AnalysisResult) -> bool {
    !visible_child_edges(node_id, result).is_empty()
}

fn child_edges<'a>(node_id: &str, result: &'a AnalysisResult) -> Vec<&'a crate::graph::GraphEdge> {
    let mut edges: Vec<_> = result
        .edges
        .iter()
        .filter(|edge| edge.from == node_id)
        .collect();
    edges.sort_by(|a, b| {
        a.to.cmp(&b.to)
            .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
    });
    edges
}

fn visible_child_edges(node_id: &str, result: &AnalysisResult) -> Vec<VisibleEdge> {
    if find_node(result, node_id).is_none() {
        return Vec::new();
    }

    let mut visible = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = outgoing_seeds(node_id, result);

    while let Some((current_id, kinds, ambiguous)) = stack.pop() {
        let Some(node) = find_node(result, &current_id) else {
            continue;
        };

        // Export-level nodes are annotations, not stops: a consumer's own
        // out-edges hang off its file node, so re-anchor there. Without this,
        // chains that arrive at `export:consumer#name` (named re-exports,
        // CommonJS re-exports) dead-end and the consumer's downstream paths
        // silently vanish from the cascade.
        if node.kind == NodeKind::Export {
            let owner_id = rebuilt_file_id(&node.file);
            if owner_id != node_id && owner_id != current_id {
                stack.push((owner_id, kinds, ambiguous));
            }
            continue;
        }

        let kind = final_visible_kind(&kinds);
        let key = format!("{}:{:?}:{}", current_id, kind, ambiguous);
        if seen.insert(key) {
            visible.push(VisibleEdge {
                to: current_id,
                kind,
                is_ambiguous: ambiguous,
            });
        }
    }

    visible.sort_by(|a, b| {
        a.to.cmp(&b.to)
            .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
    });
    visible
}

/// The out-edges to walk when expanding a node. Edges are emitted at mixed
/// granularity — most hops have `file:` parents, but single-export roots emit
/// from the `export:` node — so a file node's true child set is the union of
/// its own out-edges and those of its export-level nodes.
fn outgoing_seeds(node_id: &str, result: &AnalysisResult) -> Vec<(String, Vec<EdgeKind>, bool)> {
    let mut ids = vec![node_id.to_string()];
    if let Some(node) = find_node(result, node_id).filter(|node| node.kind == NodeKind::File) {
        ids.extend(
            result
                .nodes
                .iter()
                .filter(|other| other.kind == NodeKind::Export && other.file == node.file)
                .map(|other| other.id.clone()),
        );
    }

    let mut seeds = Vec::new();
    for id in &ids {
        seeds.extend(
            child_edges(id, result)
                .into_iter()
                .map(|edge| (edge.to.clone(), vec![edge.kind], edge.is_ambiguous)),
        );
    }
    seeds
}

fn final_visible_kind(kinds: &[EdgeKind]) -> EdgeKind {
    kinds
        .iter()
        .rev()
        .copied()
        .find(|kind| !matches!(kind, EdgeKind::ReexportsNamed | EdgeKind::ReexportsStar))
        .unwrap_or_else(|| kinds.last().copied().unwrap_or(EdgeKind::ReexportsNamed))
}

/// Traversal state shared across path branches: `path` guards against cycles
/// on the current root-to-leaf walk; `expanded` remembers nodes whose subtree
/// has already been printed anywhere in the tree.
#[derive(Default)]
struct BranchWalk {
    path: BTreeSet<String>,
    expanded: BTreeSet<String>,
}

fn render_path_branch(
    node_id: &str,
    result: &AnalysisResult,
    prefix: &str,
    is_last: bool,
    walk: &mut BranchWalk,
    lines: &mut Vec<String>,
    theme: &Theme,
) {
    let Some(node) = find_node(result, node_id) else {
        return;
    };

    let branch = if is_last { "└── " } else { "├── " };

    // A node reachable along several paths gets its subtree printed once;
    // later occurrences collapse to a back-reference instead of repeating it.
    let already_expanded = walk.expanded.contains(node_id);
    let has_children = !visible_child_edges(node_id, result).is_empty();

    let suffix = if already_expanded && has_children {
        format!("  {}", theme.muted("(paths shown above)"))
    } else {
        String::new()
    };

    let edge_summary = edge_summary(node_id, result, theme);
    lines.push(format!(
        "{}{}{}{}{}",
        prefix,
        theme.muted(branch),
        format_node(node, theme),
        edge_summary,
        suffix
    ));

    if already_expanded {
        return;
    }
    walk.expanded.insert(node_id.to_string());

    if !walk.path.insert(node_id.to_string()) {
        return;
    }

    let next_prefix = if is_last {
        format!("{prefix}    ")
    } else {
        format!("{prefix}│   ")
    };

    let child_edges = visible_child_edges(node_id, result);
    for (index, edge) in child_edges.iter().enumerate() {
        let is_last_child = index + 1 == child_edges.len();
        if walk.path.contains(&edge.to) {
            continue;
        }
        render_path_branch(
            &edge.to,
            result,
            &next_prefix,
            is_last_child,
            walk,
            lines,
            theme,
        );
    }

    walk.path.remove(node_id);
}

fn edge_summary(node_id: &str, result: &AnalysisResult, theme: &Theme) -> String {
    let mut labels = Vec::new();
    for edge in result.edges.iter().filter(|edge| edge.to == node_id) {
        labels.push(edge_label(edge.kind, edge.is_ambiguous, theme));
    }
    labels.sort();
    labels.dedup();

    if labels.is_empty() {
        String::new()
    } else {
        format!("  {}", labels.join(" "))
    }
}

fn direct_child_line(edge: &VisibleEdge, result: &AnalysisResult, theme: &Theme) -> String {
    let child = find_node(result, &edge.to)
        .map(|node| format_node(node, theme))
        .unwrap_or_else(|| theme.muted(&edge.to));
    format!(
        "{} {} {}",
        theme.direct("direct"),
        child,
        edge_label(edge.kind, edge.is_ambiguous, theme)
    )
}

#[derive(Debug, Clone)]
struct VisibleEdge {
    to: String,
    kind: EdgeKind,
    is_ambiguous: bool,
}

fn edge_label(kind: EdgeKind, is_ambiguous: bool, theme: &Theme) -> String {
    let base = match kind {
        EdgeKind::ImportsNamed => "named import",
        EdgeKind::ImportsDefault => "default import",
        EdgeKind::ImportsNamespace => "namespace import",
        EdgeKind::ImportsDynamic => "dynamic import",
        EdgeKind::ReexportsNamed => "re-export",
        EdgeKind::ReexportsStar => "export *",
        EdgeKind::UsesJsxComponent => "component use",
        EdgeKind::RequiresModule => "require",
        EdgeKind::CommonJsExport => "re-exported local",
    };

    if is_ambiguous {
        theme.warn_tag(format!("{base}?"))
    } else {
        theme.edge_tag(base)
    }
}

fn format_node(node: &GraphNode, theme: &Theme) -> String {
    let icon = match node.kind {
        NodeKind::File => "ƒ",
        NodeKind::Export => "⇢",
    };

    match node.kind {
        NodeKind::File => {
            let depth = if node.depth == 0 {
                theme.depth_root("root")
            } else {
                theme.depth(node.depth)
            };
            format!("{} {} {}", theme.file(icon), theme.path(&node.label), depth)
        }
        NodeKind::Export => {
            let (file, symbol) = split_export_label(&node.label);
            format!(
                "{} {} {}",
                theme.export(icon),
                theme.path(file),
                theme.symbol(symbol.unwrap_or(""))
            )
        }
    }
}

fn split_export_label(label: &str) -> (&str, Option<&str>) {
    if let Some((file, symbol)) = label.rsplit_once('#') {
        (file, Some(symbol))
    } else {
        (label, None)
    }
}

#[derive(Default)]
struct NodeCounts {
    files: usize,
    exports: usize,
}

fn count_node_kinds(result: &AnalysisResult) -> NodeCounts {
    let mut counts = NodeCounts::default();
    for node in &result.nodes {
        match node.kind {
            NodeKind::File => counts.files += 1,
            NodeKind::Export => counts.exports += 1,
        }
    }
    counts
}
