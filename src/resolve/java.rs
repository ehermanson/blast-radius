use std::path::{Path, PathBuf};

use super::Resolver;

impl Resolver {
    pub(super) fn resolve_java_import(&self, specifier: &str) -> Option<PathBuf> {
        if specifier.ends_with(".*") {
            let package_path = specifier.trim_end_matches(".*").replace('.', "/");
            return self
                .java_package_index
                .get(&PathBuf::from(package_path))
                .and_then(|files| files.first().cloned());
        }

        self.suffix_index
            .get(&PathBuf::from(format!(
                "{}.java",
                specifier.replace('.', "/")
            )))
            .cloned()
    }
}

pub(super) fn is_java_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("java")
}
