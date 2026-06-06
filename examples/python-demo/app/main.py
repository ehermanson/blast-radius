from app.models import User
from app.services import send_email


def register_user(email: str) -> User:
    user = User(email=email)
    send_email(user, "welcome")
    return user
