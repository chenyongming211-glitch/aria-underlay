"""YANG schema collection via NETCONF get-schema (RFC 6022).

Downloads YANG module text from devices and manages a local YANG library
at ``data/yang-library/{vendor}/{model}/{os_version}/``.

The collection is read-only and safe to run during capability probing.
Devices that lack get-schema support produce skipped entries instead
of raising errors.
"""
from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path

from aria_underlay_adapter.model_profile import extract_yang_modules_from_capabilities


YANG_LIBRARY_RELATIVE_DIR = "data/yang-library"
YANG_INDEX_FILENAME = "yang-modules.json"
GET_SCHEMA_NOT_SUPPORTED_HINTS = (
    "operation-not-supported",
    "unknown-element",
    "unknown-namespace",
    "missing-element",
)


@dataclass(frozen=True)
class YangSchemaResult:
    """Result of a single YANG module schema download attempt."""

    name: str
    revision: str
    namespace: str
    schema_text: str
    schema_size_bytes: int
    schema_downloaded: bool
    format: str
    error: str = ""

    def to_summary_dict(self) -> dict:
        return {
            "name": self.name,
            "revision": self.revision,
            "namespace": self.namespace,
            "schema_size_bytes": self.schema_size_bytes,
            "schema_downloaded": self.schema_downloaded,
            "format": self.format,
            "error": self.error,
        }


@dataclass(frozen=True)
class YangCollectionResult:
    """Aggregate result of collecting schemas for all advertised modules."""

    modules: list[YangSchemaResult] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    @property
    def downloaded_count(self) -> int:
        return sum(1 for module in self.modules if module.schema_downloaded)

    @property
    def skipped_count(self) -> int:
        return sum(1 for module in self.modules if not module.schema_downloaded)

    def to_summary_dicts(self) -> list[dict]:
        return [module.to_summary_dict() for module in self.modules]


def collect_yang_schemas(
    session,
    raw_capabilities: list[str],
    *,
    max_modules: int = 500,
) -> YangCollectionResult:
    """Download YANG schemas via NETCONF get-schema for each advertised module.

    Args:
        session: An open ncclient manager session.
        raw_capabilities: The raw NETCONF server capabilities list.
        max_modules: Safety cap on the number of modules to probe.

    Returns:
        A YangCollectionResult with per-module download results.
    """
    modules = extract_yang_modules_from_capabilities(raw_capabilities)
    if not modules:
        return YangCollectionResult(
            warnings=["no YANG modules found in NETCONF capabilities"]
        )

    if len(modules) > max_modules:
        trimmed = dict(list(modules.items())[:max_modules])
        warning = (
            f"YANG module count ({len(modules)}) exceeds safety cap "
            f"({max_modules}); collecting first {max_modules} only"
        )
        modules = trimmed
    else:
        warning = ""

    results: list[YangSchemaResult] = []
    warnings: list[str] = []
    if warning:
        warnings.append(warning)

    for name, revision in sorted(modules.items()):
        result = _download_single_schema(session, name, revision)
        results.append(result)
        if not result.schema_downloaded and result.error:
            warnings.append(f"get-schema skipped {name}: {result.error}")

    return YangCollectionResult(modules=results, warnings=warnings)


def _download_single_schema(
    session,
    name: str,
    revision: str,
) -> YangSchemaResult:
    """Attempt to download one YANG module via get-schema."""
    try:
        kwargs: dict = {"identifier": name}
        if revision:
            kwargs["version"] = revision
        kwargs["format"] = "yang"
        rpc_reply = session.get_schema(**kwargs)
    except Exception as exc:
        return YangSchemaResult(
            name=name,
            revision=revision,
            namespace="",
            schema_text="",
            schema_size_bytes=0,
            schema_downloaded=False,
            format="yang",
            error=_schema_error_summary(exc),
        )

    schema_text = _extract_schema_text(rpc_reply)
    namespace = _extract_namespace(schema_text)

    return YangSchemaResult(
        name=name,
        revision=revision,
        namespace=namespace,
        schema_text=schema_text,
        schema_size_bytes=len(schema_text.encode("utf-8")),
        schema_downloaded=True,
        format="yang",
    )


def _extract_schema_text(rpc_reply) -> str:
    """Extract schema text from ncclient get-schema RPC reply.

    ncclient returns the schema in ``rpc_reply.data_xml`` or
    ``rpc_reply.data_ele`` depending on version. We try the text
    attribute first (common path), then data_xml, then fall back to str.
    """
    if hasattr(rpc_reply, "data"):
        data = rpc_reply.data
        if isinstance(data, str):
            return data
        if hasattr(data, "text"):
            text = data.text
            if isinstance(text, str):
                return text

    if hasattr(rpc_reply, "data_xml"):
        return str(rpc_reply.data_xml)

    return str(rpc_reply)


def _extract_namespace(schema_text: str) -> str:
    """Extract the YANG module namespace from the schema text.

    Looks for ``namespace "..."`` or ``namespace '...'`` in the first
    2000 characters. Handles both multi-line and single-line schemas.
    """
    if not schema_text:
        return ""
    import re
    header = schema_text[:2000]
    match = re.search(r'namespace\s+["\']([^"\']+)["\']', header)
    if match:
        return match.group(1)
    return ""


def _schema_error_summary(exc: Exception) -> str:
    """Produce a short error summary from a get-schema exception."""
    message = str(exc).strip()
    if message:
        return message[:200]
    return exc.__class__.__name__


def save_yang_library(
    collection: YangCollectionResult,
    *,
    vendor: str,
    model: str,
    os_version: str,
    base_dir: str | None = None,
) -> Path:
    """Persist collected YANG schemas to the local library directory.

    Directory layout::

        {base_dir}/{vendor}/{model}/{os_version}/
            yang-modules.json
            {name}@{revision}.yang
            ...

    Args:
        collection: The result from :func:`collect_yang_schemas`.
        vendor: Device vendor (e.g. ``h3c``).
        model: Device model (e.g. ``S5560``).
        os_version: Device OS version (e.g. ``Comware7``).
        base_dir: Override for the library root. Defaults to
            ``data/yang-library`` relative to the project root.

    Returns:
        The path to the library directory for this device.
    """
    library_dir = _library_dir_for_device(
        vendor=vendor,
        model=model,
        os_version=os_version,
        base_dir=base_dir,
    )
    library_dir.mkdir(parents=True, exist_ok=True)

    for module in collection.modules:
        if not module.schema_downloaded or not module.schema_text:
            continue
        filename = _yang_filename(module.name, module.revision)
        (library_dir / filename).write_text(
            module.schema_text, encoding="utf-8"
        )

    index = {
        "vendor": vendor,
        "model": model,
        "os_version": os_version,
        "modules": collection.to_summary_dicts(),
    }
    index_path = library_dir / YANG_INDEX_FILENAME
    index_path.write_text(
        json.dumps(index, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )

    return library_dir


def load_yang_library(
    *,
    vendor: str,
    model: str,
    os_version: str,
    base_dir: str | None = None,
) -> YangCollectionResult | None:
    """Load a previously saved YANG library from disk.

    Returns ``None`` if the library does not exist for this device.
    """
    library_dir = _library_dir_for_device(
        vendor=vendor,
        model=model,
        os_version=os_version,
        base_dir=base_dir,
    )
    index_path = library_dir / YANG_INDEX_FILENAME
    if not index_path.exists():
        return None

    index = json.loads(index_path.read_text(encoding="utf-8"))
    modules: list[YangSchemaResult] = []
    for entry in index.get("modules", []):
        name = entry.get("name", "")
        revision = entry.get("revision", "")
        schema_text = ""
        if entry.get("schema_downloaded", False):
            filename = _yang_filename(name, revision)
            schema_path = library_dir / filename
            if schema_path.exists():
                schema_text = schema_path.read_text(encoding="utf-8")
        modules.append(
            YangSchemaResult(
                name=name,
                revision=revision,
                namespace=entry.get("namespace", ""),
                schema_text=schema_text,
                schema_size_bytes=entry.get("schema_size_bytes", 0),
                schema_downloaded=entry.get("schema_downloaded", False),
                format=entry.get("format", "yang"),
                error=entry.get("error", ""),
            )
        )

    return YangCollectionResult(modules=modules)


def _library_dir_for_device(
    *,
    vendor: str,
    model: str,
    os_version: str,
    base_dir: str | None,
) -> Path:
    root = Path(base_dir) if base_dir else _default_library_root()
    safe_vendor = _safe_path_component(vendor)
    safe_model = _safe_path_component(model)
    safe_version = _safe_path_component(os_version)
    return root / safe_vendor / safe_model / safe_version


def _default_library_root() -> Path:
    project_root = Path(__file__).resolve().parent.parent.parent.parent
    return project_root / YANG_LIBRARY_RELATIVE_DIR


def _safe_path_component(value: str) -> str:
    """Sanitize a vendor/model/version string for use as a directory name."""
    cleaned = value.strip().replace("/", "_").replace("\\", "_")
    if not cleaned:
        return "unknown"
    return cleaned


def _yang_filename(name: str, revision: str) -> str:
    if revision:
        return f"{name}@{revision}.yang"
    return f"{name}.yang"


def collect_and_save_yang_schemas(
    session,
    raw_capabilities: list[str],
    *,
    yang_library_dir: str | None = None,
    vendor: str = "unknown",
    model: str = "unknown",
    os_version: str = "unknown",
) -> list[dict]:
    """Collect YANG schemas from a live NETCONF session and optionally save.

    Schema collection failures are non-fatal: any exception is caught and
    reported as an empty result. Always returns the per-module summary list.
    """
    try:
        collection = collect_yang_schemas(session, raw_capabilities)
    except Exception:
        return []

    if yang_library_dir:
        try:
            save_yang_library(
                collection,
                vendor=vendor,
                model=model,
                os_version=os_version,
                base_dir=yang_library_dir,
            )
        except Exception:
            pass

    return collection.to_summary_dicts()
