pub fn format_subject(email: &str, template: &str) -> String {
    format!("{template}:{email}")
}
