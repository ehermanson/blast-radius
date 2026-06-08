use std::collections::{BTreeMap, BTreeSet};

use crate::graph::{
    AnalysisResult, AnalysisTarget, GraphNode, NodeKind, RootImpact, Workspace, compute_tier,
    package_key,
};

use super::theme::{RiskTier, Theme};

mod cascade;
use cascade::{format_mode, is_leaf, render_cascade};

pub(super) fn render_tree(result: &AnalysisResult, verbose: bool) -> String {
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
        tier: result.summary.risk_tier,
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
