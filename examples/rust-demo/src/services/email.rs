use crate::models::User;
use crate::utils::formatting::format_subject;

pub fn send_email(user: &User, template: &str) -> String {
    format_subject(&user.email, template)
}
