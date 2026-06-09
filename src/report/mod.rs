use anyhow::Result;

use crate::cli::OutputFormat;
use crate::graph::AnalysisResult;

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
        OutputFormat::Tree => render_tree(result, verbose, color),
        OutputFormat::Json => serde_json::to_string_pretty(result)?,
        OutputFormat::Mermaid => render_mermaid(result),
        OutputFormat::Dot => render_dot(result),
    };

    Ok(rendered)
}
