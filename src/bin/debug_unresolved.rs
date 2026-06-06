use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use blast_radius::fs::RepoContext;
use blast_radius::parse::parse_module;
use blast_radius::resolve::{Resolution, Resolver};

fn main() -> Result<()> {
    let repo_root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let context = RepoContext::discover(&repo_root)?;
    let resolver = Resolver::new(&context)?;

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for file in &context.source_files {
        let Ok(module) = parse_module(file) else {
            continue;
        };

        for import in &module.imports {
            if !resolver.is_internal_specifier(&module.file, &import.source) {
                continue;
            }

            if matches!(
                resolver.resolve(&module.file, &import.source),
                Resolution::Unresolved
            ) {
                *counts.entry(import.source.clone()).or_default() += 1;
            }
        }
    }

    let mut entries: Vec<_> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    for (source, count) in entries {
        println!("{count:>4} {source}");
    }

    Ok(())
}
