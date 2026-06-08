use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub mode: AnalysisMode,
    pub target: AnalysisTarget,
    pub repo_root: PathBuf,
    pub source_file_count: usize,
    pub summary: Summary,
    pub workspaces: Vec<Workspace>,
    /// Per-input-file impact, populated only for multi-file runs.
    pub roots: Vec<RootImpact>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub warnings: Vec<String>,
}

/// A workspace package discovered in the repo, used to group impacted files by
/// the package they live in.
#[derive(Debug, Clone, Serialize)]
pub struct Workspace {
    pub name: String,
    /// Package root, relative to the repo root (empty string == repo root).
    pub root: String,
}

/// The blast radius of a single input file, so a multi-file run can show each
/// file's impact individually alongside the combined total.
#[derive(Debug, Clone, Serialize)]
pub struct RootImpact {
    pub file: String,
    pub affected: usize,
    pub direct: usize,
    pub indirect: usize,
    pub max_depth: usize,
    pub packages: usize,
    pub files: Vec<RootImpactFile>,
}

/// A single file impacted by a particular input file.
#[derive(Debug, Clone, Serialize)]
pub struct RootImpactFile {
    pub path: String,
    pub endpoint: bool,
}

/// Map a repo-relative path to the package that owns it: the longest matching
/// workspace root, falling back to the top-level directory. `workspaces` is
/// expected to be sorted longest-root-first.
pub fn package_key(rel_path: &str, workspaces: &[Workspace]) -> String {
    if let Some(workspace) = workspaces.iter().find(|workspace| {
        workspace.root.is_empty()
            || rel_path == workspace.root
            || rel_path.starts_with(&format!("{}/", workspace.root))
    }) {
        if workspace.root.is_empty() {
            ".".to_string()
        } else {
            workspace.root.clone()
        }
    } else {
        match rel_path.split_once('/') {
            Some((head, _)) => head.to_string(),
            None => ".".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMode {
    Export,
    File,
    Files,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnalysisTarget {
    Export { file: PathBuf, export_name: String },
    File { file: PathBuf },
    Files { files: Vec<PathBuf> },
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Summary {
    pub directly_affected_files: usize,
    pub transitively_affected_files: usize,
    pub total_affected_files: usize,
    pub unresolved_imports: usize,
    pub ambiguous_edges: usize,
    pub parse_failures: usize,
    /// Input paths passed to `files` mode that were skipped because they were
    /// missing on disk or not recognized source files.
    pub skipped_inputs: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub file: PathBuf,
    pub symbol: Option<String>,
    pub kind: NodeKind,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    File,
    Export,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub is_ambiguous: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    ImportsNamed,
    ImportsDefault,
    ImportsNamespace,
    ReexportsNamed,
    ReexportsStar,
    UsesJsxComponent,
    RequiresModule,
    CommonJsExport,
}

#[derive(Debug, Clone)]
pub struct ModuleState {
    pub file: PathBuf,
    pub public_exports: BTreeSet<String>,
    pub export_to_locals: BTreeMap<String, BTreeSet<String>>,
    pub local_to_exports: BTreeMap<String, BTreeSet<String>>,
}

impl ModuleState {
    pub fn new(file: PathBuf) -> Self {
        Self {
            file,
            public_exports: BTreeSet::new(),
            export_to_locals: BTreeMap::new(),
            local_to_exports: BTreeMap::new(),
        }
    }

    pub fn add_export(&mut self, exported: impl Into<String>, local: Option<String>) {
        let exported = exported.into();
        self.public_exports.insert(exported.clone());
        if let Some(local) = local {
            self.export_to_locals
                .entry(exported.clone())
                .or_default()
                .insert(local.clone());
            self.local_to_exports
                .entry(local)
                .or_default()
                .insert(exported);
        }
    }
}
