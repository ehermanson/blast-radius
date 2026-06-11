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
/// Mermaid/DOT identifiers allow only word characters, so non-alphanumerics
/// are mapped to `_`. That mapping is lossy — `util-x.ts` and `util.x.ts`
/// would otherwise merge into one node — so a stable fingerprint of the
/// original id is appended to keep distinct ids distinct.
fn sanitize_id(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    if cleaned == value {
        cleaned
    } else {
        format!("{cleaned}_{:08x}", fnv1a(value))
    }
}

/// FNV-1a, 32-bit: tiny, dependency-free, stable across runs and platforms.
fn fnv1a(value: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn escape_quotes(value: &str) -> String {
    value.replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::sanitize_id;

    #[test]
    fn distinct_ids_stay_distinct_after_sanitization() {
        // `-` and `.` both map to `_`; the fingerprint suffix must keep the
        // ids apart (regression: util-x.ts and util.x.ts merged into one node).
        assert_ne!(
            sanitize_id("file:/repo/src/util-x.ts"),
            sanitize_id("file:/repo/src/util.x.ts")
        );
    }

    #[test]
    fn sanitization_is_stable() {
        assert_eq!(
            sanitize_id("file:/repo/src/a.ts"),
            sanitize_id("file:/repo/src/a.ts")
        );
    }

    #[test]
    fn plain_ids_pass_through_unchanged() {
        assert_eq!(sanitize_id("empty"), "empty");
    }
}
