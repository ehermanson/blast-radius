use std::path::{Path, PathBuf};

use super::{Resolver, clean_path};

impl Resolver {
    pub(super) fn resolve_python_import(
        &self,
        importer: &Path,
        specifier: &str,
    ) -> Option<PathBuf> {
        if specifier.starts_with('.') {
            return self.resolve_python_relative_import(importer, specifier);
        }

        let candidate = self.repo_root.join(specifier.replace('.', "/"));
        self.try_resolve_python_module_candidate(&candidate)
    }

    #[cfg(feature = "python")]
    fn resolve_python_relative_import(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        let level = specifier.chars().take_while(|char| *char == '.').count();
        let remainder = specifier.trim_start_matches('.');
        let mut base = importer.parent().unwrap_or(&self.repo_root).to_path_buf();

        for _ in 1..level {
            base.pop();
        }

        let candidate = if remainder.is_empty() {
            base
        } else {
            base.join(remainder.replace('.', "/"))
        };
        self.try_resolve_python_module_candidate(&candidate)
    }

    #[cfg(feature = "python")]
    fn try_resolve_python_module_candidate(&self, candidate: &Path) -> Option<PathBuf> {
        if let Some(path) = self.try_resolve_candidate(candidate) {
            return Some(path);
        }

        let package_init = clean_path(&candidate.join("__init__.py"));
        if self.source_files.contains(&package_init) {
            return Some(package_init);
        }

        None
    }

    #[cfg(feature = "python")]
    pub(super) fn python_top_level_exists(&self, specifier: &str) -> bool {
        let Some(first) = specifier.split('.').next() else {
            return false;
        };
        if first.is_empty() {
            return false;
        }

        let module_file = clean_path(&self.repo_root.join(format!("{first}.py")));
        let package_init = clean_path(&self.repo_root.join(first).join("__init__.py"));
        self.source_files.contains(&module_file) || self.source_files.contains(&package_init)
    }
}

pub(super) fn is_python_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("py")
}
