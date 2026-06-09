from app.models import User
from app.services import send_email
from app.utils import helpers


def register_user(email: str) -> User:
    user = User(email=email)
    send_email(user, helpers.normalize_template("welcome"))
    return user
