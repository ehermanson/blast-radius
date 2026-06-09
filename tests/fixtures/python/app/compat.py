from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.models import User

try:
    from app.utils.formatting import format_subject
except ImportError:  # pragma: no cover
    def format_subject(email: str, template: str) -> str:
        return f"{template}:{email}"


def describe(user: "User") -> str:
    return format_subject(user.email, "compat")
