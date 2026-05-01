from __future__ import annotations

import os
from pathlib import Path
import tempfile
from typing import Callable

from aria_underlay_adapter.errors import AdapterError


def connect_with_known_hosts_file(manager, connect_args, path):
    validate_known_hosts_path(path)

    with tempfile.NamedTemporaryFile("w", encoding="utf-8") as ssh_config:
        ssh_config.write("Host *\n")
        ssh_config.write(f"  UserKnownHostsFile {path}\n")
        ssh_config.flush()
        strict_args = dict(connect_args)
        strict_args["ssh_config"] = ssh_config.name
        return manager.connect(**strict_args)


def connect_with_tofu(
    manager,
    connect_args,
    *,
    host: str,
    port: int,
    known_hosts_path: str,
    connect_strict: Callable | None = None,
):
    validate_known_hosts_path(known_hosts_path)
    store = KnownHostsTrustStore(known_hosts_path)
    strict_connect = connect_strict or connect_with_known_hosts_file
    if store.has_host(host, port):
        return strict_connect(manager, connect_args, known_hosts_path)

    first_use_args = dict(connect_args)
    first_use_args["hostkey_verify"] = False
    session = manager.connect(**first_use_args)
    try:
        key_name, key_b64 = remote_host_key(session)
        store.trust(host, port, key_name, key_b64)
    except AdapterError:
        close_session(session)
        raise
    except Exception as exc:
        close_session(session)
        raise AdapterError(
            code="HOST_KEY_TRUST_STORE_WRITE_FAILED",
            message="failed to persist TOFU host key",
            normalized_error="tofu trust store write failed",
            raw_error_summary=str(exc),
            retryable=False,
        ) from exc
    return session


class KnownHostsTrustStore:
    def __init__(self, path: str):
        self.path = Path(path)

    def has_host(self, host: str, port: int) -> bool:
        return self._find_line(host, port) is not None

    def trust(self, host: str, port: int, key_name: str, key_b64: str) -> None:
        host_pattern = known_hosts_pattern(host, port)
        trusted_line = f"{host_pattern} {key_name} {key_b64}"
        existing = self._find_line(host, port)
        if existing is not None:
            if existing.strip() == trusted_line:
                return
            raise AdapterError(
                code="HOST_KEY_CHANGED",
                message="TOFU host key does not match existing trust store entry",
                normalized_error="tofu host key changed",
                raw_error_summary=f"host={host_pattern}",
                retryable=False,
            )

        lines = self._read_lines()
        lines.append(f"{trusted_line}\n")
        payload = "".join(lines)
        try:
            atomic_write_text(self.path, payload)
        except OSError as exc:
            raise AdapterError(
                code="HOST_KEY_TRUST_STORE_WRITE_FAILED",
                message="failed to persist TOFU host key",
                normalized_error="tofu trust store write failed",
                raw_error_summary=str(exc),
                retryable=False,
            ) from exc

    def _find_line(self, host: str, port: int) -> str | None:
        host_pattern = known_hosts_pattern(host, port)
        for line in self._read_lines():
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            fields = stripped.split()
            if not fields:
                continue
            hosts = fields[0].split(",")
            if host_pattern in hosts:
                return stripped
        return None

    def _read_lines(self) -> list[str]:
        if not self.path.exists():
            return []
        return self.path.read_text(encoding="utf-8").splitlines(keepends=True)


def validate_known_hosts_path(path: str) -> None:
    if not path or "\n" in path or "\r" in path:
        raise AdapterError(
            code="HOST_KEY_POLICY_INVALID",
            message="known_hosts path contains invalid characters",
            normalized_error="invalid known_hosts path",
            raw_error_summary="known_hosts path must be a non-empty single-line filesystem path",
            retryable=False,
        )


def known_hosts_pattern(host: str, port: int) -> str:
    if port == 22:
        return host
    return f"[{host}]:{port}"


def remote_host_key(session) -> tuple[str, str]:
    for owner in (
        getattr(session, "_session", None),
        getattr(session, "session", None),
        session,
    ):
        if owner is None:
            continue
        transport = getattr(owner, "_transport", None) or getattr(owner, "transport", None)
        if transport is None or not hasattr(transport, "get_remote_server_key"):
            continue
        key = transport.get_remote_server_key()
        if key is None:
            continue
        key_name = key.get_name()
        key_b64 = key.get_base64()
        if key_name and key_b64:
            return key_name, key_b64

    raise AdapterError(
        code="HOST_KEY_UNAVAILABLE",
        message="NETCONF session did not expose a remote host key for TOFU",
        normalized_error="remote host key unavailable",
        raw_error_summary="session transport has no remote server key",
        retryable=False,
    )


def close_session(session) -> None:
    close = getattr(session, "close_session", None)
    if callable(close):
        close()
        return
    close = getattr(session, "close", None)
    if callable(close):
        close()


def atomic_write_text(path: Path, payload: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp_path = None
    try:
        with tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as handle:
            temp_path = Path(handle.name)
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_path, path)
        directory_fd = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    except Exception:
        if temp_path is not None:
            try:
                temp_path.unlink()
            except FileNotFoundError:
                pass
        raise
