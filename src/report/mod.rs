use anyhow::Result;

use crate::cli::OutputFormat;
use crate::graph::{AnalysisMode, AnalysisResult};

mod graph_formats;
use graph_formats::{render_dot, render_mermaid};

mod theme;

mod tree;
use tree::render_tree;

pub fn render(
    format: &OutputFormat,
    result: &AnalysisResult,
    verbose: bool,
    color: bool,
) -> Result<String> {
    let rendered = match format {
        // The risk-verdict tree is meaningless for a whole-repo graph dump, so
        // fall back to a plain importer-direction edge listing.
        OutputFormat::Tree if matches!(result.mode, AnalysisMode::Graph) => {
            render_graph_edge_list(result)
        }
        OutputFormat::Tree => render_tree(result, verbose, color),
        OutputFormat::Json => serde_json::to_string_pretty(result)?,
        OutputFormat::Mermaid => render_mermaid(result),
        OutputFormat::Dot => render_dot(result),
    };

    Ok(rendered)
}

/// A plain `importer -> importee` listing for `graph` in the default (tree)
/// format. Edges are stored depended-upon -> consumer, so flip for display.
fn render_graph_edge_list(result: &AnalysisResult) -> String {
    use std::collections::BTreeMap;

    let labels: BTreeMap<&str, &str> = result
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.label.as_str()))
        .collect();
    let mut lines: Vec<String> = result
        .edges
        .iter()
        .map(|edge| {
            let importer = labels.get(edge.to.as_str()).copied().unwrap_or(&edge.to);
            let importee = labels
                .get(edge.from.as_str())
                .copied()
                .unwrap_or(&edge.from);
            format!("{importer} -> {importee}")
        })
        .collect();
    lines.sort();
    if lines.is_empty() {
        format!("{} files, no import edges", result.source_file_count)
    } else {
        lines.join("\n")
    }
}
