from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from aria_underlay_adapter.errors import AdapterError


@dataclass(frozen=True)
class NetconfSecret:
    username: str
    password: str | None = None
    key_path: str | None = None
    passphrase: str | None = None


class LocalSecretProvider:
    def __init__(
        self,
        secret_file: str | None = None,
        env_prefix: str = "ARIA_UNDERLAY_SECRET",
    ):
        self._secret_file = secret_file
        self._env_prefix = env_prefix

    def resolve(self, secret_ref: str) -> NetconfSecret:
        if not secret_ref:
            raise _missing_secret_error(secret_ref, "empty secret_ref")

        env_secret = self._resolve_from_env(secret_ref)
        if env_secret is not None:
            return env_secret

        file_secret = self._resolve_from_file(secret_ref)
        if file_secret is not None:
            return file_secret

        raise _missing_secret_error(
            secret_ref,
            "no matching environment variables or local secret file entry",
        )

    def _resolve_from_env(self, secret_ref: str) -> NetconfSecret | None:
        key = _secret_ref_env_key(secret_ref)
        username = os.getenv(f"{self._env_prefix}_{key}_USERNAME")
        password = os.getenv(f"{self._env_prefix}_{key}_PASSWORD")
        key_path = os.getenv(f"{self._env_prefix}_{key}_KEY_PATH")
        passphrase = os.getenv(f"{self._env_prefix}_{key}_PASSPHRASE")

        if username is None and password is None and key_path is None:
            return None

        return _secret_from_mapping(
            secret_ref,
            {
                "username": username,
                "password": password,
                "key_path": key_path,
                "passphrase": passphrase,
            },
        )

    def _resolve_from_file(self, secret_ref: str) -> NetconfSecret | None:
        if not self._secret_file:
            return None

        path = Path(self._secret_file)
        try:
            content = path.read_text(encoding="utf-8")
            document = json.loads(content)
        except FileNotFoundError as exc:
            raise AdapterError(
                code="SECRET_FILE_NOT_FOUND",
                message=f"secret file not found: {path}",
                normalized_error="secret file missing",
                raw_error_summary=str(exc),
                retryable=False,
            ) from exc
        except json.JSONDecodeError as exc:
            raise AdapterError(
                code="SECRET_FILE_INVALID",
                message=f"secret file is not valid JSON: {path}",
                normalized_error="secret file invalid",
                raw_error_summary=str(exc),
                retryable=False,
            ) from exc

        if not isinstance(document, dict):
            raise AdapterError(
                code="SECRET_FILE_INVALID",
                message="secret file root must be a JSON object",
                normalized_error="secret file invalid",
                raw_error_summary="root is not object",
                retryable=False,
            )

        secrets = document.get("secrets", document)
        if not isinstance(secrets, dict):
            raise AdapterError(
                code="SECRET_FILE_INVALID",
                message="secret file secrets field must be a JSON object",
                normalized_error="secret file invalid",
                raw_error_summary="secrets is not object",
                retryable=False,
            )

        entry = secrets.get(secret_ref)
        if entry is None:
            return None
        if not isinstance(entry, dict):
            raise AdapterError(
                code="SECRET_ENTRY_INVALID",
                message=f"secret entry must be an object: {secret_ref}",
                normalized_error="secret entry invalid",
                raw_error_summary=secret_ref,
                retryable=False,
            )

        return _secret_from_mapping(secret_ref, entry)


def _secret_from_mapping(secret_ref: str, mapping: dict[str, Any]) -> NetconfSecret:
    username = mapping.get("username")
    password = mapping.get("password")
    key_path = mapping.get("key_path")
    passphrase = mapping.get("passphrase")

    if not isinstance(username, str) or not username:
        raise _missing_secret_error(secret_ref, "missing username")
    if password is not None and not isinstance(password, str):
        raise _invalid_secret_error(secret_ref, "password must be a string")
    if key_path is not None and not isinstance(key_path, str):
        raise _invalid_secret_error(secret_ref, "key_path must be a string")
    if passphrase is not None and not isinstance(passphrase, str):
        raise _invalid_secret_error(secret_ref, "passphrase must be a string")
    if not password and not key_path:
        raise _missing_secret_error(secret_ref, "missing password or key_path")

    return NetconfSecret(
        username=username,
        password=password,
        key_path=key_path,
        passphrase=passphrase,
    )


def _secret_ref_env_key(secret_ref: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "_", secret_ref).strip("_").upper()


def _missing_secret_error(secret_ref: str, reason: str) -> AdapterError:
    return AdapterError(
        code="SECRET_NOT_FOUND",
        message=f"secret not found or incomplete: {secret_ref}",
        normalized_error="secret missing",
        raw_error_summary=reason,
        retryable=False,
    )


def _invalid_secret_error(secret_ref: str, reason: str) -> AdapterError:
    return AdapterError(
        code="SECRET_INVALID",
        message=f"secret is invalid: {secret_ref}",
        normalized_error="secret invalid",
        raw_error_summary=reason,
        retryable=False,
    )
