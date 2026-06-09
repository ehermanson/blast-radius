use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModuleFacts {
    pub file: PathBuf,
    pub exports: Vec<ExportFact>,
    pub imports: Vec<ImportFact>,
    pub reexports: Vec<ReexportFact>,
    pub used_locals: BTreeSet<String>,
    pub jsx_locals: BTreeSet<String>,
    pub namespace_member_usage: BTreeMap<String, BTreeSet<String>>,
    pub jsx_namespace_member_usage: BTreeMap<String, BTreeSet<String>>,
    pub warnings: Vec<String>,
}

impl ModuleFacts {
    pub(super) fn empty(file: &Path) -> Self {
        Self {
            file: file.to_path_buf(),
            exports: Vec::new(),
            imports: Vec::new(),
            reexports: Vec::new(),
            used_locals: BTreeSet::new(),
            jsx_locals: BTreeSet::new(),
            namespace_member_usage: BTreeMap::new(),
            jsx_namespace_member_usage: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportFact {
    pub exported: String,
    pub local: Option<String>,
    pub kind: ExportKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Local,
    Default,
    Reexport,
    CommonJs,
}

#[derive(Debug, Clone)]
pub struct ImportFact {
    pub source: String,
    pub local: String,
    pub imported: ImportTarget,
    pub kind: ImportKind,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
    Name(String),
    Default,
    Namespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Esm,
    CommonJs,
    /// A dynamic `import("...")` call, e.g. a lazy route or code-split component.
    Dynamic,
}

#[derive(Debug, Clone)]
pub struct ReexportFact {
    pub source: String,
    pub imported: ReexportTarget,
    pub exported: String,
    pub is_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReexportTarget {
    Name(String),
    Default,
    Namespace,
    All,
}
