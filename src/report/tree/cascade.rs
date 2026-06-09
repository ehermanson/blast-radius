use std::collections::BTreeSet;
use std::path::Path;

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
            for (index, child_id) in cascading_children.iter().enumerate() {
                let is_last_branch = index + 1 == cascading_children.len();
                let mut path = BTreeSet::new();
                path.insert(root_id.clone());
                render_path_branch(
                    child_id,
                    result,
                    "",
                    is_last_branch,
                    &mut path,
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

fn preferred_root(result: &AnalysisResult) -> Option<String> {
    match &result.target {
        AnalysisTarget::Export { file, .. } => {
            let file_id = format!("file:{}", file.display());
            find_existing_node(result, &[&file_id])
        }
        AnalysisTarget::File { file } => {
            let file_id = format!("file:{}", file.display());
            find_existing_node(result, &[&file_id])
        }
        AnalysisTarget::Files { files } => {
            let preferred: Vec<String> = files
                .iter()
                .map(|file| format!("file:{}", file.display()))
                .collect();
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
    let Some(root_node) = find_node(result, node_id) else {
        return Vec::new();
    };

    let mut visible = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = child_edges(node_id, result)
        .into_iter()
        .map(|edge| (edge.to.clone(), vec![edge.kind], edge.is_ambiguous))
        .collect::<Vec<_>>();

    while let Some((current_id, kinds, ambiguous)) = stack.pop() {
        let Some(node) = find_node(result, &current_id) else {
            continue;
        };

        if is_transparent_node(node, root_node, result) {
            for edge in child_edges(&current_id, result) {
                let mut next_kinds = kinds.clone();
                next_kinds.push(edge.kind);
                stack.push((edge.to.clone(), next_kinds, ambiguous || edge.is_ambiguous));
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

fn final_visible_kind(kinds: &[EdgeKind]) -> EdgeKind {
    kinds
        .iter()
        .rev()
        .copied()
        .find(|kind| !matches!(kind, EdgeKind::ReexportsNamed | EdgeKind::ReexportsStar))
        .unwrap_or_else(|| kinds.last().copied().unwrap_or(EdgeKind::ReexportsNamed))
}

fn is_transparent_node(node: &GraphNode, root_node: &GraphNode, result: &AnalysisResult) -> bool {
    if node.id == root_node.id {
        return false;
    }

    match node.kind {
        NodeKind::Export => true,
        NodeKind::File => is_barrel_passthrough(node, result),
    }
}

fn is_barrel_passthrough(node: &GraphNode, result: &AnalysisResult) -> bool {
    if file_stem(&node.file) != Some("index") {
        return false;
    }

    let mut incoming = result
        .edges
        .iter()
        .filter(|edge| edge.to == node.id)
        .peekable();
    let mut outgoing = result
        .edges
        .iter()
        .filter(|edge| edge.from == node.id)
        .peekable();

    if incoming.peek().is_none() || outgoing.peek().is_none() {
        return false;
    }

    incoming.all(|edge| {
        matches!(
            edge.kind,
            EdgeKind::ReexportsNamed | EdgeKind::ReexportsStar
        )
    })
}

fn file_stem(path: &Path) -> Option<&str> {
    path.file_stem().and_then(|stem| stem.to_str())
}

fn render_path_branch(
    node_id: &str,
    result: &AnalysisResult,
    prefix: &str,
    is_last: bool,
    path: &mut BTreeSet<String>,
    lines: &mut Vec<String>,
    theme: &Theme,
) {
    let Some(node) = find_node(result, node_id) else {
        return;
    };

    let branch = if is_last { "└── " } else { "├── " };

    let edge_summary = edge_summary(node_id, result, theme);
    lines.push(format!(
        "{}{}{}{}",
        prefix,
        theme.muted(branch),
        format_node(node, theme),
        edge_summary
    ));

    if !path.insert(node_id.to_string()) {
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
        if path.contains(&edge.to) {
            continue;
        }
        render_path_branch(
            &edge.to,
            result,
            &next_prefix,
            is_last_child,
            path,
            lines,
            theme,
        );
    }

    path.remove(node_id);
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
