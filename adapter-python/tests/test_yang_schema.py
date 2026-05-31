"""Tests for YANG schema collection via NETCONF get-schema (RFC 6022)."""
from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from aria_underlay_adapter.backends.yang_schema import (
    YangCollectionResult,
    YangSchemaResult,
    collect_yang_schemas,
    load_yang_library,
    save_yang_library,
)


H3C_LIKE_CAPABILITIES = [
    "urn:ietf:params:netconf:base:1.0",
    "urn:ietf:params:netconf:base:1.1",
    "http://www.h3c.com/netconf/base:1.0?module=h3c-vlan&revision=2021-07-15",
    "http://www.h3c.com/netconf/base:1.0?module=h3c-if&revision=2021-05-10",
    "urn:ietf:params:xml:ns:yang:ietf-interfaces?module=ietf-interfaces&revision=2014-05-08",
]


class FakeRpcReply:
    """Minimal stand-in for ncclient RPC reply with ``.data`` attribute."""

    def __init__(self, text: str):
        self.data = text


class FakeGetSchemaError(Exception):
    """Simulates ncclient raising on unknown get-schema operation."""


def _mock_session_with_schemas(schemas: dict[str, str]):
    """Return a mock ncclient session that responds to get_schema by name."""
    session = MagicMock()

    def _get_schema(identifier, version=None, format=None):
        if identifier in schemas:
            return FakeRpcReply(schemas[identifier])
        raise FakeGetSchemaError(f"unknown module {identifier}")

    session.get_schema.side_effect = _get_schema
    return session


def test_collect_yang_schemas_downloads_advertised_modules():
    schemas = {
        "h3c-vlan": 'module h3c-vlan { namespace "http://www.h3c.com/yang/vlan"; }',
        "h3c-if": 'module h3c-if { namespace "http://www.h3c.com/yang/if"; }',
        "ietf-interfaces": 'module ietf-interfaces { namespace "urn:ietf:params:xml:ns:yang:ietf-interfaces"; }',
    }
    session = _mock_session_with_schemas(schemas)

    result = collect_yang_schemas(session, H3C_LIKE_CAPABILITIES)

    assert result.downloaded_count == 3
    assert result.skipped_count == 0
    names = {m.name for m in result.modules}
    assert names == {"h3c-vlan", "h3c-if", "ietf-interfaces"}
    for module in result.modules:
        assert module.schema_downloaded is True
        assert module.schema_size_bytes > 0
        assert module.namespace  # each schema text declares a namespace


def test_collect_yang_schemas_records_errors_for_unsupported_modules():
    session = MagicMock()
    session.get_schema.side_effect = FakeGetSchemaError("operation-not-supported")

    result = collect_yang_schemas(session, H3C_LIKE_CAPABILITIES)

    assert result.downloaded_count == 0
    assert result.skipped_count == 3
    for module in result.modules:
        assert module.schema_downloaded is False
        assert "operation-not-supported" in module.error


def test_collect_yang_schemas_handles_partial_success():
    schemas = {
        "h3c-vlan": 'module h3c-vlan { namespace "http://www.h3c.com/yang/vlan"; }',
    }
    session = _mock_session_with_schemas(schemas)

    result = collect_yang_schemas(session, H3C_LIKE_CAPABILITIES)

    assert result.downloaded_count == 1
    assert result.skipped_count == 2
    downloaded = next(m for m in result.modules if m.name == "h3c-vlan")
    assert downloaded.schema_downloaded is True
    skipped = [m for m in result.modules if not m.schema_downloaded]
    assert len(skipped) == 2


def test_collect_yang_schemas_empty_capabilities_returns_warning():
    session = MagicMock()

    result = collect_yang_schemas(session, ["urn:ietf:params:netconf:base:1.0"])

    assert result.downloaded_count == 0
    assert any("no YANG modules" in w for w in result.warnings)


def test_save_and_load_yang_library_roundtrip(tmp_path: Path):
    collection = YangCollectionResult(
        modules=[
            YangSchemaResult(
                name="h3c-vlan",
                revision="2021-07-15",
                namespace="http://www.h3c.com/yang/vlan",
                schema_text='module h3c-vlan { namespace "http://www.h3c.com/yang/vlan"; }',
                schema_size_bytes=59,
                schema_downloaded=True,
                format="yang",
            ),
            YangSchemaResult(
                name="ietf-interfaces",
                revision="2014-05-08",
                namespace="urn:ietf:params:xml:ns:yang:ietf-interfaces",
                schema_text="",
                schema_size_bytes=0,
                schema_downloaded=False,
                format="yang",
                error="operation-not-supported",
            ),
        ]
    )

    library_dir = save_yang_library(
        collection,
        vendor="h3c",
        model="S5560",
        os_version="Comware7",
        base_dir=str(tmp_path),
    )

    assert (library_dir / "h3c-vlan@2021-07-15.yang").exists()
    assert not (library_dir / "ietf-interfaces@2014-05-08.yang").exists()
    index = json.loads((library_dir / "yang-modules.json").read_text())
    assert index["vendor"] == "h3c"
    assert index["model"] == "S5560"
    assert len(index["modules"]) == 2

    loaded = load_yang_library(
        vendor="h3c",
        model="S5560",
        os_version="Comware7",
        base_dir=str(tmp_path),
    )

    assert loaded is not None
    assert len(loaded.modules) == 2
    vlan_module = next(m for m in loaded.modules if m.name == "h3c-vlan")
    assert vlan_module.schema_downloaded is True
    assert "h3c-vlan" in vlan_module.schema_text
    ietf_module = next(m for m in loaded.modules if m.name == "ietf-interfaces")
    assert ietf_module.schema_downloaded is False
    assert ietf_module.error == "operation-not-supported"


def test_load_yang_library_returns_none_when_missing(tmp_path: Path):
    result = load_yang_library(
        vendor="huawei",
        model="CE6800",
        os_version="VRP8",
        base_dir=str(tmp_path),
    )
    assert result is None


def test_yang_schema_result_to_summary_dict():
    module = YangSchemaResult(
        name="openconfig-interfaces",
        revision="2023-02-06",
        namespace="http://openconfig.net/yang/interfaces",
        schema_text="module openconfig-interfaces { ... }",
        schema_size_bytes=36,
        schema_downloaded=True,
        format="yang",
    )

    summary = module.to_summary_dict()

    assert summary["name"] == "openconfig-interfaces"
    assert summary["revision"] == "2023-02-06"
    assert summary["schema_downloaded"] is True
    assert summary["schema_size_bytes"] == 36


def test_collect_yang_schemas_caps_module_count():
    caps = [
        "urn:ietf:params:netconf:base:1.0",
    ] + [
        f"http://example.com/yang?module=mod-{i}&revision=2024-01-01"
        for i in range(10)
    ]
    session = MagicMock()
    session.get_schema.return_value = FakeRpcReply(
        'module example { namespace "http://example.com/yang"; }'
    )

    result = collect_yang_schemas(session, caps, max_modules=5)

    assert session.get_schema.call_count == 5
    assert any("safety cap" in w for w in result.warnings)
