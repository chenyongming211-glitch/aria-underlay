import sys
from types import SimpleNamespace

from aria_underlay_adapter.backends.gnmi_capabilities import (
    GnmiCapabilityProbe,
    parse_gnmi_capabilities,
)


def test_parse_gnmi_capabilities_extracts_models_and_encodings():
    result = parse_gnmi_capabilities(
        {
            "supported_models": [
                {
                    "name": "openconfig-bgp",
                    "organization": "OpenConfig",
                    "version": "2024-10-30",
                },
                {
                    "name": "openconfig-network-instance",
                    "organization": "OpenConfig",
                    "version": "2024-10-30",
                },
            ],
            "supported_encodings": ["json_ietf", "proto"],
        }
    )

    assert [model["name"] for model in result.supported_models] == [
        "openconfig-bgp",
        "openconfig-network-instance",
    ]
    assert result.supported_models[0]["version"] == "2024-10-30"
    assert result.encodings == ["json_ietf", "proto"]
    assert result.raw_capabilities["supported_encodings"] == ["json_ietf", "proto"]


def test_gnmi_capability_probe_uses_injected_client_factory():
    client = _FakeGnmiClient(
        {
            "supported_models": [
                {"name": "openconfig-interfaces", "version": "2024-10-30"},
            ],
            "supported_encodings": ["json_ietf"],
        }
    )
    probe = GnmiCapabilityProbe(
        host="192.0.2.10",
        port=57400,
        username="netconf",
        password="secret",
        tls_enabled=True,
        client_factory=lambda: client,
    )

    result = probe.get_capabilities()

    assert client.entered is True
    assert result.supported_models == [
        {"name": "openconfig-interfaces", "organization": "", "version": "2024-10-30"}
    ]
    assert result.encodings == ["json_ietf"]


def test_gnmi_capability_probe_builds_pygnmi_client_kwargs(monkeypatch):
    calls = []

    def fake_gnmi_client(**kwargs):
        calls.append(kwargs)
        return _FakeGnmiClient(
            {
                "supported_models": [
                    {"name": "openconfig-bgp", "version": "2024-10-30"},
                ],
                "supported_encodings": ["json_ietf"],
            }
        )

    monkeypatch.setitem(sys.modules, "pygnmi", SimpleNamespace())
    monkeypatch.setitem(
        sys.modules,
        "pygnmi.client",
        SimpleNamespace(gNMIclient=fake_gnmi_client),
    )
    probe = GnmiCapabilityProbe(
        host="192.0.2.10",
        port=57401,
        username="netconf",
        password="secret",
        tls_enabled=True,
        tls_ca_cert_file="/etc/aria/gnmi-ca.pem",
        tls_cert_file="/etc/aria/gnmi-client.pem",
        tls_key_file="/etc/aria/gnmi-client.key",
        timeout_secs=7,
    )

    result = probe.get_capabilities()

    assert calls == [
        {
            "target": ("192.0.2.10", 57401),
            "username": "netconf",
            "password": "secret",
            "insecure": False,
            "gnmi_timeout": 7,
            "path_root": "/etc/aria/gnmi-ca.pem",
            "path_cert": "/etc/aria/gnmi-client.pem",
            "path_key": "/etc/aria/gnmi-client.key",
        }
    ]
    assert result.supported_models[0]["name"] == "openconfig-bgp"


class _FakeGnmiClient:
    def __init__(self, capabilities):
        self.capabilities_payload = capabilities
        self.entered = False

    def __enter__(self):
        self.entered = True
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def capabilities(self):
        return self.capabilities_payload
