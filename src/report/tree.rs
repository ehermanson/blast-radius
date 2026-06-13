use std::collections::{BTreeMap, BTreeSet};

use crate::graph::{
    AnalysisResult, AnalysisTarget, GraphNode, NodeKind, RootImpact, Workspace, compute_tier,
    normalize_separators, package_key,
};

use super::theme::{RiskTier, Theme};

mod cascade;
use cascade::{format_mode, is_leaf, render_cascade};

pub(super) fn render_tree(result: &AnalysisResult, verbose: bool, color: bool) -> String {
    let theme = Theme::new(color);
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

    // Where the impact lands: directory counts give the shape of the blast at
    // a glance, before the reader commits to the full file list.
    if assessment.affected >= HOTSPOT_MIN_FILES {
        let hotspots = hotspot_dirs(result);
        if hotspots.len() >= 2 {
            lines.push(String::new());
            lines.push(theme.rule("hotspots"));
            render_hotspots(&hotspots, assessment.tier, &theme, &mut lines);
        }
    }

    // Past this size a per-file list stops being readable, so collapse to the
    // directory rollups unless the user explicitly asks for everything.
    let list_files = verbose || assessment.affected <= MAX_LISTED_FILES;
    if multi {
        // Attribute impacted files to the input that caused them.
        lines.push(String::new());
        lines.push(theme.rule(&format!("impact by input file · {}", result.roots.len())));
        for (index, root) in result.roots.iter().enumerate() {
            if index > 0 {
                lines.push(String::new());
            }
            render_root_block(root, &result.workspaces, list_files, &theme, &mut lines);
        }
        push_list_caveats(assessment.affected > 0, list_files, &theme, &mut lines);
    } else {
        // Single change: one list, grouped by package, then by directory.
        let groups = group_by_package(result);
        if !groups.is_empty() {
            lines.push(String::new());
            lines.push(theme.rule(&format!(
                "impacted files · {} in {} package{}",
                assessment.affected,
                assessment.packages,
                plural(assessment.packages)
            )));
            render_package_groups(&groups, 2, list_files, &theme, &mut lines);
            push_list_caveats(true, list_files, &theme, &mut lines);
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
        AnalysisTarget::Graph => None,
    };
    file.map(|file| {
        normalize_separators(
            file.strip_prefix(&result.repo_root)
                .unwrap_or(file)
                .display()
                .to_string(),
        )
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
        AnalysisTarget::Graph => "the import graph".to_string(),
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
    list_files: bool,
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
    render_package_groups(&groups, 4, list_files, theme, lines);
}

/// Render package groups at a given indent: each package header, then its
/// directories busiest-first, then (unless collapsed) the files themselves as
/// bare names — the directory header already carries the shared prefix.
fn render_package_groups(
    groups: &[PackageGroup],
    indent: usize,
    list_files: bool,
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
        for dir in dir_groups(group) {
            lines.push(format!(
                "{pad}  {} {}",
                theme.path(&dir.label),
                theme.muted(&format!("({})", dir.files.len()))
            ));
            if !list_files {
                continue;
            }
            for (file, name) in &dir.files {
                let marker = if file.endpoint {
                    format!("  {}", theme.endpoint("◎ endpoint"))
                } else {
                    String::new()
                };
                lines.push(format!("{pad}    {}{}", theme.path(name), marker));
            }
        }
    }
}

/// The how-to-expand note under a collapsed file list.
fn push_list_caveats(any_files: bool, list_files: bool, theme: &Theme, lines: &mut Vec<String>) {
    if !any_files || list_files {
        return;
    }
    lines.push(String::new());
    lines.push(format!(
        "  {}",
        theme.muted(&format!(
            "file lists collapsed past {MAX_LISTED_FILES} impacted files · -v to list every file"
        ))
    ));
}

struct PackageGroup {
    label: String,
    files: Vec<ImpactedFile>,
}

/// One directory's worth of a package's impacted files: the directory label
/// (relative to the package) and each file paired with its bare name.
struct DirGroup<'a> {
    label: String,
    files: Vec<(&'a ImpactedFile, String)>,
}

/// Bucket a package's files by directory, busiest directory first.
fn dir_groups(group: &PackageGroup) -> Vec<DirGroup<'_>> {
    let prefix = if group.label == "." {
        String::new()
    } else {
        format!("{}/", group.label)
    };

    let mut buckets: BTreeMap<String, Vec<(&ImpactedFile, String)>> = BTreeMap::new();
    for file in &group.files {
        let local = file.path.strip_prefix(&prefix).unwrap_or(&file.path);
        let (dir, name) = split_dir(local);
        buckets
            .entry(dir.to_string())
            .or_default()
            .push((file, name.to_string()));
    }

    let mut dirs: Vec<DirGroup> = buckets
        .into_iter()
        .map(|(dir, files)| DirGroup {
            label: dir_label(&dir),
            files,
        })
        .collect();
    dirs.sort_by(|a, b| {
        b.files
            .len()
            .cmp(&a.files.len())
            .then(a.label.cmp(&b.label))
    });
    dirs
}

/// Impacted-file lists longer than this collapse to directory rollups unless
/// `-v` is passed — past that size the per-file list stops being readable.
const MAX_LISTED_FILES: usize = 200;

/// How many directories the hotspot chart shows.
const HOTSPOT_ROWS: usize = 6;

/// Below this many impacted files the list itself is glanceable, so a hotspot
/// chart would just repeat it.
const HOTSPOT_MIN_FILES: usize = 8;

/// Affected-file counts per directory, busiest first — the shape of the blast.
fn hotspot_dirs(result: &AnalysisResult) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for node in result
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File && node.depth >= 1)
    {
        let (dir, _) = split_dir(&node.label);
        *counts.entry(dir.to_string()).or_default() += 1;
    }
    let mut dirs: Vec<(String, usize)> = counts.into_iter().collect();
    dirs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    dirs
}

fn render_hotspots(
    dirs: &[(String, usize)],
    tier: RiskTier,
    theme: &Theme,
    lines: &mut Vec<String>,
) {
    const BAR_CELLS: usize = 14;
    const LABEL_MAX: usize = 36;

    let shown = &dirs[..dirs.len().min(HOTSPOT_ROWS)];
    let max = shown.iter().map(|(_, count)| *count).max().unwrap_or(1);
    let labels: Vec<String> = shown
        .iter()
        .map(|(dir, _)| clip_left(&dir_label(dir), LABEL_MAX))
        .collect();
    let width = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);

    for ((_, count), label) in shown.iter().zip(&labels) {
        let filled = (count * BAR_CELLS).div_ceil(max).min(BAR_CELLS);
        lines.push(format!(
            "  {}{}  {} {}",
            theme.path(label),
            " ".repeat(width - label.chars().count()),
            theme.hotspot_bar(tier, filled, BAR_CELLS),
            theme.count(&format!("{:>3}", count)),
        ));
    }
    if dirs.len() > shown.len() {
        lines.push(format!(
            "  {}",
            theme.muted(&format!("+{} more directories", dirs.len() - shown.len()))
        ));
    }
}

/// Split a path into its directory and bare file name. Labels are normalized
/// to `/` at creation, but accept `\` too in case a Windows-style path slips
/// through.
fn split_dir(path: &str) -> (&str, &str) {
    path.rsplit_once(['/', '\\']).unwrap_or(("", path))
}

fn dir_label(dir: &str) -> String {
    if dir.is_empty() {
        "./".to_string()
    } else {
        format!("{dir}/")
    }
}

/// Truncate from the left, keeping the most specific path segments.
fn clip_left(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count + 1 - max).collect();
    format!("…{tail}")
}

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

#[cfg(test)]
mod tests {
    use super::split_dir;

    #[test]
    fn split_dir_handles_forward_slashes() {
        assert_eq!(split_dir("src/report/tree.rs"), ("src/report", "tree.rs"));
        assert_eq!(split_dir("main.rs"), ("", "main.rs"));
    }

    #[test]
    fn split_dir_handles_backslashes() {
        assert_eq!(
            split_dir("src\\report\\tree.rs"),
            ("src\\report", "tree.rs")
        );
        // Mixed separators: split at the last separator of either kind.
        assert_eq!(split_dir("src/report\\tree.rs"), ("src/report", "tree.rs"));
        assert_eq!(split_dir("src\\report/tree.rs"), ("src\\report", "tree.rs"));
    }
}
