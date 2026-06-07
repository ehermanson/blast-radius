use std::path::{Path, PathBuf};

use super::Resolver;

impl Resolver {
    pub(super) fn resolve_ruby_import(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        if specifier.starts_with('.') {
            let base = importer.parent().unwrap_or(&self.repo_root);
            return self.try_resolve_candidate(&base.join(specifier));
        }

        for candidate in [
            self.repo_root.join(specifier),
            self.repo_root.join("lib").join(specifier),
            self.repo_root.join("app").join(specifier),
        ] {
            if let Some(path) = self.try_resolve_candidate(&candidate) {
                return Some(path);
            }
        }

        self.suffix_index
            .get(&PathBuf::from(format!("{specifier}.rb")))
            .cloned()
    }
}

pub(super) fn is_ruby_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("rb")
}
