use std::path::{Path, PathBuf};

use super::{Resolver, clean_path};

impl Resolver {
    pub(super) fn resolve_rust_import(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        if let Some(module) = specifier.strip_prefix("mod:") {
            let base = rust_child_module_base(importer);
            return self.try_resolve_rust_module_candidate(&base.join(module));
        }

        let parts: Vec<&str> = specifier
            .split("::")
            .filter(|part| !part.is_empty())
            .collect();
        let (head, rest) = parts.split_first()?;

        match *head {
            "crate" => self.resolve_rust_from_crate_roots(rest),
            "self" => {
                let base = rust_child_module_base(importer).join(rest.join("/"));
                self.try_resolve_rust_module_candidate(&base)
            }
            "super" => {
                let mut base = rust_parent_module_base(importer);
                for part in rest {
                    if *part == "super" {
                        base.pop();
                    } else {
                        base.push(part);
                    }
                }
                self.try_resolve_rust_module_candidate(&base)
            }
            _ => {
                if let Some(path) = self.resolve_rust_from_crate_roots(&parts) {
                    return Some(path);
                }
                let base = rust_child_module_base(importer).join(parts.join("/"));
                self.try_resolve_rust_module_candidate(&base)
            }
        }
    }

    #[cfg(feature = "rust")]
    fn resolve_rust_from_crate_roots(&self, parts: &[&str]) -> Option<PathBuf> {
        if parts.is_empty() {
            return None;
        }

        for root in self.rust_crate_roots() {
            let candidate = root.join(parts.join("/"));
            if let Some(path) = self.try_resolve_rust_module_candidate(&candidate) {
                return Some(path);
            }
        }
        None
    }

    #[cfg(feature = "rust")]
    fn rust_crate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for file in &self.source_files {
            let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if matches!(name, "lib.rs" | "main.rs")
                && let Some(parent) = file.parent()
            {
                roots.push(parent.to_path_buf());
            }
        }
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            roots.push(self.repo_root.clone());
        }
        roots
    }

    #[cfg(feature = "rust")]
    fn try_resolve_rust_module_candidate(&self, candidate: &Path) -> Option<PathBuf> {
        if let Some(path) = self.try_resolve_candidate(candidate) {
            return Some(path);
        }

        let mod_file = clean_path(&candidate.join("mod.rs"));
        if self.source_files.contains(&mod_file) {
            return Some(mod_file);
        }

        None
    }

    #[cfg(feature = "rust")]
    pub(super) fn rust_top_level_exists(&self, specifier: &str) -> bool {
        let Some(first) = specifier.split("::").next() else {
            return false;
        };
        if first.is_empty() {
            return false;
        }

        self.rust_crate_roots().into_iter().any(|root| {
            self.try_resolve_rust_module_candidate(&root.join(first))
                .is_some()
        })
    }
}

pub(super) fn is_rust_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("rs")
}

pub(super) fn rust_child_module_base(importer: &Path) -> PathBuf {
    let parent = importer.parent().unwrap_or_else(|| Path::new(""));
    match importer.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") => parent.to_path_buf(),
        _ => parent.join(importer.file_stem().unwrap_or_default()),
    }
}

pub(super) fn rust_parent_module_base(importer: &Path) -> PathBuf {
    let child_base = rust_child_module_base(importer);
    child_base
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(child_base)
}
