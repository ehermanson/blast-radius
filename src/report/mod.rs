use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;

use crate::cli::OutputFormat;
use crate::graph::{
    AnalysisMode, AnalysisResult, AnalysisTarget, EdgeKind, GraphNode, NodeKind, RootImpact,
    Workspace, package_key,
};

mod graph_formats;
use graph_formats::{render_dot, render_mermaid};

mod theme;
use theme::{RiskTier, Theme};

pub fn render(format: &OutputFormat, result: &AnalysisResult, verbose: bool) -> Result<String> {
    let rendered = match format {
        OutputFormat::Tree => render_tree(result, verbose),
        OutputFormat::Json => serde_json::to_string_pretty(result)?,
        OutputFormat::Mermaid => render_mermaid(result),
        OutputFormat::Dot => render_dot(result),
    };

    Ok(rendered)
}

fn render_tree(result: &AnalysisResult, verbose: bool) -> String {
    let theme = Theme::detect();
    let assessment = assess(result);
    let mut lines = Vec::new();

    // ── Brand ──────────────────────────────────────────────
    for line in theme.banner() {
        lines.push(line);
    }
    lines.push(format!(
        "  {}",
        theme.muted(&format!(
            "impact analysis · {} mode",
            format_mode(&result.mode)
        ))
    ));
    lines.push(String::new());

    // ── Target ─────────────────────────────────────────────
    let multi = result.roots.len() > 1;
    if multi {
        let header = format!(
            "  {}",
            theme.subject(&format!("{} input files", result.roots.len()))
        );
        lines.push(header);
    } else {
        lines.push(format!(
            "  {}   {}",
            theme.subject(&format_subject(&result.target)),
            theme.path(&relative_target(result))
        ));
    }
    lines.push(String::new());

    // ── Verdict ────────────────────────────────────────────
    if assessment.affected == 0 {
        lines.push(format!(
            "  {}  {}",
            theme.risk_pill(RiskTier::Minor),
            theme.subject("nothing depends on this — safe to change")
        ));
    } else {
        let aggregate = if multi {
            format!("  (across all {} inputs)", result.roots.len())
        } else {
            String::new()
        };
        lines.push(format!(
            "  {}  {}  {}{}",
            theme.risk_pill(assessment.tier),
            theme.meter(assessment.tier),
            theme.subject(&format!(
                "{} impacted file{} · {} package{}",
                assessment.affected,
                plural(assessment.affected),
                assessment.packages,
                plural(assessment.packages)
            )),
            theme.muted(&aggregate)
        ));
        lines.push(format!(
            "  {}",
            theme.muted(&format!(
                "{} direct, {} indirect · depth {} · {} endpoint{}",
                result.summary.directly_affected_files,
                result.summary.transitively_affected_files,
                assessment.max_depth,
                assessment.leaves,
                plural(assessment.leaves),
            ))
        ));
    }

    if multi {
        // Attribute impacted files to the input that caused them.
        lines.push(String::new());
        lines.push(theme.rule(&format!("impact by input file · {}", result.roots.len())));
        for (index, root) in result.roots.iter().enumerate() {
            if index > 0 {
                lines.push(String::new());
            }
            render_root_block(root, &result.workspaces, &theme, &mut lines);
        }
    } else {
        // Single change: one flat list, grouped by package.
        let groups = group_by_package(result);
        if !groups.is_empty() {
            lines.push(String::new());
            lines.push(theme.rule(&format!(
                "impacted files · {} in {} package{}",
                assessment.affected,
                assessment.packages,
                plural(assessment.packages)
            )));
            render_package_groups(&groups, 2, &theme, &mut lines);
        }
    }

    if !result.warnings.is_empty() {
        lines.push(String::new());
        lines.push(theme.rule("warnings"));
        for warning in &result.warnings {
            lines.push(format!("  {} {}", theme.warn("!"), theme.warn(warning)));
        }
    }

    if verbose {
        render_cascade(result, &theme, &mut lines);
    }

    // ── Footer ─────────────────────────────────────────────
    lines.push(String::new());
    let mut footer = format!(
        "{} · {} scanned",
        confidence_tag(&assessment, &theme),
        theme.muted(&format!("{} files", result.source_file_count)),
    );
    if !verbose && assessment.affected > 0 {
        footer.push_str(&format!(" · {}", theme.muted("-v for full cascade")));
    }
    lines.push(format!("  {footer}"));

    lines.join("\n")
}

/// The detailed root → direct → cascade tree, shown only with `--verbose`.
fn render_cascade(result: &AnalysisResult, theme: &Theme, lines: &mut Vec<String>) {
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

struct Assessment {
    tier: RiskTier,
    /// Downstream files that depend on the target (excludes the target itself).
    affected: usize,
    /// Distinct packages those files live in.
    packages: usize,
    /// Files at the end of the chain (nothing depends on them in turn).
    leaves: usize,
    max_depth: usize,
    ambiguous: usize,
    unresolved: usize,
    parse_failures: usize,
}

fn assess(result: &AnalysisResult) -> Assessment {
    let affected_nodes: Vec<&GraphNode> = result
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File && node.depth >= 1)
        .collect();

    let affected = affected_nodes.len();
    let max_depth = affected_nodes
        .iter()
        .map(|node| node.depth)
        .max()
        .unwrap_or(0);
    let leaves = affected_nodes
        .iter()
        .filter(|node| is_leaf(&node.id, result))
        .count();

    let mut package_keys = BTreeSet::new();
    for node in &affected_nodes {
        package_keys.insert(package_key(&node.label, &result.workspaces));
    }
    let packages = package_keys.len();

    // Ambiguity scoped to edges actually traversed for *this* impact, so the
    // confidence reflects this result — not unrelated barrels elsewhere.
    let ambiguous = result.edges.iter().filter(|edge| edge.is_ambiguous).count();

    Assessment {
        tier: compute_tier(affected, packages),
        affected,
        packages,
        leaves,
        max_depth,
        ambiguous,
        // Unresolved imports have unknown targets, so they can't be scoped to a
        // path — they're a repo-wide blind spot that may hide extra consumers.
        unresolved: result.summary.unresolved_imports,
        parse_failures: result.summary.parse_failures,
    }
}

/// Reach and spread drive the tier; ambiguity is surfaced as a confidence
/// caveat rather than inflating the score, so the headline stays trustworthy.
fn compute_tier(affected: usize, packages: usize) -> RiskTier {
    if affected == 0 {
        RiskTier::Minor
    } else if affected > 25 || packages >= 3 {
        RiskTier::High
    } else if affected <= 3 && packages <= 1 {
        RiskTier::Minor
    } else if affected <= 10 && packages <= 2 {
        RiskTier::Moderate
    } else {
        RiskTier::Risky
    }
}

/// The target path, relative to the repo root when possible.
fn relative_target(result: &AnalysisResult) -> String {
    let file = match &result.target {
        AnalysisTarget::Export { file, .. } => Some(file),
        AnalysisTarget::File { file } => Some(file),
        AnalysisTarget::Files { files } => files.first(),
    };
    file.map(|file| {
        file.strip_prefix(&result.repo_root)
            .unwrap_or(file)
            .display()
            .to_string()
    })
    .unwrap_or_default()
}

fn format_subject(target: &AnalysisTarget) -> String {
    match target {
        AnalysisTarget::Export { export_name, .. } => export_name.clone(),
        AnalysisTarget::File { file } => file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("this file")
            .to_string(),
        AnalysisTarget::Files { files } => match files.split_first() {
            Some((only, [])) => only
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("this file")
                .to_string(),
            _ => format!("{} files", files.len()),
        },
    }
}

/// A single impacted file: its repo-relative path and whether it's an endpoint
/// (a leaf that nothing else depends on — the thing that ultimately ships).
struct ImpactedFile {
    path: String,
    endpoint: bool,
}

/// One input file's block: a header (severity + the file + its reach) followed
/// by the files it impacts, grouped by package.
fn render_root_block(
    root: &RootImpact,
    workspaces: &[Workspace],
    theme: &Theme,
    lines: &mut Vec<String>,
) {
    let tier = compute_tier(root.affected, root.packages);
    let reach = if root.affected == 0 {
        "no dependents — safe to change".to_string()
    } else {
        format!(
            "{} file{} impacted · depth {}",
            root.affected,
            plural(root.affected),
            root.max_depth
        )
    };
    lines.push(format!(
        "  {} {}  {}",
        theme.tier_dot(tier),
        theme.subject(&root.file),
        theme.muted(&format!("— {reach}")),
    ));

    let groups = group_files(
        root.files.iter().map(|file| ImpactedFile {
            path: file.path.clone(),
            endpoint: file.endpoint,
        }),
        workspaces,
    );
    render_package_groups(&groups, 4, theme, lines);
}

/// Render package groups (header + file paths) at a given indent.
fn render_package_groups(
    groups: &[PackageGroup],
    indent: usize,
    theme: &Theme,
    lines: &mut Vec<String>,
) {
    let pad = " ".repeat(indent);
    for group in groups {
        lines.push(format!(
            "{pad}{} {}",
            theme.pkg(&group.label),
            theme.count(&format!("({})", group.files.len()))
        ));
        for file in group.files.iter().take(FILES_PER_PACKAGE) {
            let marker = if file.endpoint {
                format!("  {}", theme.endpoint("◎ endpoint"))
            } else {
                String::new()
            };
            lines.push(format!("{pad}  {}{}", theme.path(&file.path), marker));
        }
        if group.files.len() > FILES_PER_PACKAGE {
            lines.push(format!(
                "{pad}  {}",
                theme.muted(&format!("+{} more", group.files.len() - FILES_PER_PACKAGE))
            ));
        }
    }
}

struct PackageGroup {
    label: String,
    files: Vec<ImpactedFile>,
}

/// The number of impacted files to list per package before collapsing the rest.
const FILES_PER_PACKAGE: usize = 12;

fn group_by_package(result: &AnalysisResult) -> Vec<PackageGroup> {
    let files = result
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File && node.depth >= 1)
        .map(|node| ImpactedFile {
            path: node.label.clone(),
            endpoint: is_leaf(&node.id, result),
        });
    group_files(files, &result.workspaces)
}

/// Bucket impacted files by the package that owns them, widest package first.
fn group_files(
    files: impl IntoIterator<Item = ImpactedFile>,
    workspaces: &[Workspace],
) -> Vec<PackageGroup> {
    let mut buckets: BTreeMap<String, Vec<ImpactedFile>> = BTreeMap::new();
    for file in files {
        let label = package_key(&file.path, workspaces);
        buckets.entry(label).or_default().push(file);
    }

    let mut groups: Vec<PackageGroup> = buckets
        .into_iter()
        .map(|(label, mut files)| {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            files.dedup_by(|a, b| a.path == b.path);
            PackageGroup { label, files }
        })
        .collect();

    groups.sort_by(|a, b| {
        b.files
            .len()
            .cmp(&a.files.len())
            .then(a.label.cmp(&b.label))
    });
    groups
}

fn is_leaf(node_id: &str, result: &AnalysisResult) -> bool {
    !result.edges.iter().any(|edge| edge.from == node_id)
}

/// A compact confidence tag for the footer.
///
/// The high/partial verdict is driven only by ambiguity *on the impacted paths*
/// — so "partial" means this specific result was traced through edges the
/// analyzer couldn't pin down, not that the repo has fuzzy bits elsewhere.
/// Repo-wide unresolved imports are surfaced as a separate "may hide consumers"
/// caveat, since their targets are unknown and can't be tied to this path.
fn confidence_tag(assessment: &Assessment, theme: &Theme) -> String {
    let on_path_clean = assessment.affected == 0 || assessment.ambiguous == 0;

    let mut tag = if on_path_clean {
        format!("{} {}", theme.ok("●"), theme.muted("confidence: high"))
    } else {
        format!(
            "{} {}",
            theme.warn("●"),
            theme.warn(&format!(
                "confidence: partial · {} ambiguous edge{} on these paths",
                assessment.ambiguous,
                plural(assessment.ambiguous)
            ))
        )
    };

    if assessment.affected > 0 && assessment.unresolved > 0 {
        tag.push_str(&theme.muted(&format!(
            " · {} unresolved import{} repo-wide may hide consumers",
            assessment.unresolved,
            plural(assessment.unresolved)
        )));
    }

    if assessment.parse_failures > 0 {
        tag.push_str(&theme.muted(&format!(
            " · {} parse failure{} caused skipped file{} repo-wide and may hide consumers",
            assessment.parse_failures,
            plural(assessment.parse_failures),
            plural(assessment.parse_failures)
        )));
    }

    tag
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
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

fn format_mode(mode: &AnalysisMode) -> &'static str {
    match mode {
        AnalysisMode::Export => "export",
        AnalysisMode::File => "file",
        AnalysisMode::Files => "files",
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

#[allow(dead_code)]
fn _node_label<'a>(nodes: &'a [GraphNode], id: &str) -> Option<&'a str> {
    nodes
        .iter()
        .find(|node| node.id == id)
        .map(|node| node.label.as_str())
}

#[allow(dead_code)]
fn _basename(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}
