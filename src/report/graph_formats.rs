use crate::graph::AnalysisResult;

pub(super) fn render_mermaid(result: &AnalysisResult) -> String {
    let mut lines = vec!["graph TD".to_string()];

    if result.nodes.is_empty() {
        lines.push("    empty[\"No affected files found\"]".to_string());
        return lines.join("\n");
    }

    for node in &result.nodes {
        lines.push(format!(
            "    {}[\"{}\"]",
            sanitize_id(&node.id),
            escape_quotes(&node.label)
        ));
    }

    for edge in &result.edges {
        lines.push(format!(
            "    {} -->|{}| {}",
            sanitize_id(&edge.from),
            format!("{:?}", edge.kind).to_lowercase(),
            sanitize_id(&edge.to)
        ));
    }

    lines.join("\n")
}

pub(super) fn render_dot(result: &AnalysisResult) -> String {
    let mut lines = vec!["digraph blast_radius {".to_string()];

    if result.nodes.is_empty() {
        lines.push("  empty [label=\"No affected files found\"];".to_string());
        lines.push("}".to_string());
        return lines.join("\n");
    }

    for node in &result.nodes {
        lines.push(format!(
            "  {} [label=\"{}\"];",
            sanitize_id(&node.id),
            escape_quotes(&node.label)
        ));
    }

    for edge in &result.edges {
        lines.push(format!(
            "  {} -> {} [label=\"{}\"];",
            sanitize_id(&edge.from),
            sanitize_id(&edge.to),
            format!("{:?}", edge.kind).to_lowercase()
        ));
    }

    lines.push("}".to_string());
    lines.join("\n")
}
fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn escape_quotes(value: &str) -> String {
    value.replace('"', "\\\"")
}
