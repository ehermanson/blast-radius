use std::path::{Path, PathBuf};

use crate::fs::TsConfigPath;

use super::{
    Resolution, Resolver, apply_alias_target, clean_path, match_alias, package_specifier_parts,
    resolve_package_export,
};

impl Resolver {
    pub(super) fn resolve_javascript_import(&self, importer: &Path, specifier: &str) -> Resolution {
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return self.resolve_path(importer.parent().unwrap_or(&self.repo_root), specifier);
        }

        if let Some(path) = self.resolve_tsconfig_alias(importer, specifier) {
            return Resolution::Resolved(path);
        }

        if let Some(path) = self.resolve_workspace_package(specifier) {
            return Resolution::Resolved(path);
        }

        Resolution::Unresolved
    }

    pub(super) fn is_internal_javascript_specifier(
        &self,
        importer: &Path,
        specifier: &str,
    ) -> bool {
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return true;
        }

        if let Some(tsconfig) = self.nearest_tsconfig(importer)
            && tsconfig
                .compiler_options
                .paths
                .keys()
                .any(|pattern| match_alias(pattern, specifier).is_some())
        {
            return true;
        }

        package_specifier_parts(specifier)
            .map(|(package_name, _)| self.package_by_name.contains_key(package_name))
            .unwrap_or(false)
    }

    fn resolve_tsconfig_alias(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        let tsconfig = self.nearest_tsconfig(importer)?;
        let tsconfig_dir = tsconfig.path.parent()?;
        let base_dir = tsconfig
            .compiler_options
            .base_url
            .as_ref()
            .map(|base| clean_path(&tsconfig_dir.join(base)))
            .unwrap_or_else(|| tsconfig_dir.to_path_buf());

        for (pattern, targets) in &tsconfig.compiler_options.paths {
            let Some(captures) = match_alias(pattern, specifier) else {
                continue;
            };

            for target in targets {
                let candidate = apply_alias_target(target, &captures);
                if let Resolution::Resolved(resolved) = self.resolve_path(&base_dir, &candidate) {
                    return Some(resolved);
                }
            }
        }

        None
    }

    fn nearest_tsconfig(&self, importer: &Path) -> Option<&TsConfigPath> {
        self.tsconfigs
            .iter()
            .filter(|config| importer.starts_with(config.path.parent().unwrap_or(&self.repo_root)))
            .max_by_key(|config| config.path.components().count())
    }

    fn resolve_workspace_package(&self, specifier: &str) -> Option<PathBuf> {
        let (package_name, rest) = package_specifier_parts(specifier)?;
        let package = self
            .package_by_name
            .get(package_name)
            .and_then(|index| self.packages.get(*index))?;

        if let Some(rest) = rest {
            let export_key = format!("./{rest}");
            if let Some(resolved) = resolve_package_export(package, &export_key)
                .and_then(|path| self.try_resolve_candidate(&path))
            {
                return Some(resolved);
            }

            let direct = package.root.join(rest);
            if let Some(resolved) = self.try_resolve_candidate(&direct) {
                return Some(resolved);
            }

            let src_direct = package.root.join("src").join(rest);
            if let Some(resolved) = self.try_resolve_candidate(&src_direct) {
                return Some(resolved);
            }

            return None;
        }

        if let Some(resolved) =
            resolve_package_export(package, ".").and_then(|path| self.try_resolve_candidate(&path))
        {
            return Some(resolved);
        }

        for candidate in &package.entry_candidates {
            if let Some(resolved) = self.try_resolve_candidate(candidate) {
                return Some(resolved);
            }
        }

        None
    }
}
