import json

import pytest

from aria_underlay_adapter.errors import AdapterError
from aria_underlay_adapter.secret_provider import LocalSecretProvider


def test_secret_provider_resolves_env_password_secret(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_SECRET_LOCAL_TEST_DEVICE_USERNAME", "netconf")
    monkeypatch.setenv("ARIA_UNDERLAY_SECRET_LOCAL_TEST_DEVICE_PASSWORD", "secret")

    secret = LocalSecretProvider().resolve("local/test-device")

    assert secret.username == "netconf"
    assert secret.password == "secret"
    assert secret.key_path is None


def test_secret_provider_resolves_env_key_secret(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_SECRET_LOCAL_TEST_DEVICE_USERNAME", "netconf")
    monkeypatch.setenv("ARIA_UNDERLAY_SECRET_LOCAL_TEST_DEVICE_KEY_PATH", "/keys/leaf-a")
    monkeypatch.setenv("ARIA_UNDERLAY_SECRET_LOCAL_TEST_DEVICE_PASSPHRASE", "passphrase")

    secret = LocalSecretProvider().resolve("local/test-device")

    assert secret.username == "netconf"
    assert secret.key_path == "/keys/leaf-a"
    assert secret.passphrase == "passphrase"


def test_secret_provider_resolves_json_file_secret(tmp_path):
    secret_file = tmp_path / "secrets.json"
    secret_file.write_text(
        json.dumps(
            {
                "secrets": {
                    "local/test-device": {
                        "username": "netconf",
                        "password": "secret",
                    }
                }
            }
        ),
        encoding="utf-8",
    )

    secret = LocalSecretProvider(secret_file=str(secret_file)).resolve("local/test-device")

    assert secret.username == "netconf"
    assert secret.password == "secret"


def test_secret_provider_reports_missing_secret():
    with pytest.raises(AdapterError) as exc:
        LocalSecretProvider().resolve("local/missing")

    assert exc.value.code == "SECRET_NOT_FOUND"
    assert exc.value.retryable is False


def test_secret_provider_rejects_incomplete_secret(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_SECRET_LOCAL_TEST_DEVICE_USERNAME", "netconf")

    with pytest.raises(AdapterError) as exc:
        LocalSecretProvider().resolve("local/test-device")

    assert exc.value.code == "SECRET_NOT_FOUND"
    assert "password or key_path" in exc.value.raw_error_summary


def test_secret_provider_rejects_invalid_json_file(tmp_path):
    secret_file = tmp_path / "secrets.json"
    secret_file.write_text("{invalid json", encoding="utf-8")

    with pytest.raises(AdapterError) as exc:
        LocalSecretProvider(secret_file=str(secret_file)).resolve("local/test-device")

    assert exc.value.code == "SECRET_FILE_INVALID"
