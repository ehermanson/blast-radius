use std::path::{Path, PathBuf};

use anyhow::Result;

use super::javascript::{module_facts_from_javascript_module, parse_source};
use super::{ExportFact, ExportKind, ModuleFacts};

pub(crate) fn parse_component_module(path: &Path, source: &str, kind: &str) -> Result<ModuleFacts> {
    let script = extract_component_scripts(source);
    let virtual_path = component_virtual_script_path(path, &script);
    let module = parse_source(&virtual_path, &script.source)?;
    let mut facts = module_facts_from_javascript_module(path, &module)?;

    facts.exports.push(ExportFact {
        exported: "default".to_string(),
        local: None,
        kind: ExportKind::Default,
    });
    facts
        .used_locals
        .extend(facts.imports.iter().map(|import| import.local.clone()));
    facts.warnings.push(format!(
        "parsed {kind} script blocks as JavaScript/TypeScript"
    ));

    Ok(facts)
}

#[derive(Debug)]
struct ComponentScript {
    source: String,
    is_typescript: bool,
}

fn extract_component_scripts(source: &str) -> ComponentScript {
    let mut remaining = source;
    let mut scripts = Vec::new();
    let mut is_typescript = false;

    while let Some(start) = remaining.find("<script") {
        remaining = &remaining[start + "<script".len()..];
        let Some(tag_end) = remaining.find('>') else {
            break;
        };
        let attrs = &remaining[..tag_end];
        is_typescript |= component_script_is_typescript(attrs);
        remaining = &remaining[tag_end + 1..];
        let Some(script_end) = remaining.find("</script>") else {
            break;
        };
        scripts.push(remaining[..script_end].to_string());
        remaining = &remaining[script_end + "</script>".len()..];
    }

    ComponentScript {
        source: scripts.join("\n"),
        is_typescript,
    }
}

fn component_script_is_typescript(attrs: &str) -> bool {
    attrs.contains("lang=\"ts\"")
        || attrs.contains("lang='ts'")
        || attrs.contains("lang=ts")
        || attrs.contains("lang=\"tsx\"")
        || attrs.contains("lang='tsx'")
        || attrs.contains("lang=tsx")
}

fn component_virtual_script_path(path: &Path, script: &ComponentScript) -> PathBuf {
    let extension = if script.is_typescript { "ts" } else { "js" };
    path.with_extension(format!(
        "{}.{extension}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("component")
    ))
}
