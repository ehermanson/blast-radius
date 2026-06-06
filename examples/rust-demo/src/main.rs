use crate::models::User;
use crate::services::send_email;

fn main() {
    let user = User {
        email: "user@example.com".to_string(),
    };
    let _ = send_email(&user, "welcome");
}
