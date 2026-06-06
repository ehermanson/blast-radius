from ..models import User
from ..utils.formatting import format_subject


def send_email(user: User, template: str) -> str:
    return format_subject(user.email, template)
