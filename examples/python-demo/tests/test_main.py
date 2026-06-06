from app.main import register_user


def test_register_user():
    assert register_user("user@example.com").email == "user@example.com"
