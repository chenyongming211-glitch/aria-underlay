from __future__ import annotations

import time
from concurrent import futures
from pathlib import Path

import grpc
import pytest

from aria_underlay_adapter.config import AdapterConfig
from aria_underlay_adapter.server import (
    UnderlayAdapterService,
    build_server,
    build_server_credentials,
)


TLS_FIXTURES = Path(__file__).parent / "fixtures" / "tls"
CA_CERT = TLS_FIXTURES / "ca.crt"
SERVER_CERT = TLS_FIXTURES / "server.crt"
SERVER_KEY = TLS_FIXTURES / "server.key"
CLIENT_CERT = TLS_FIXTURES / "client.crt"
CLIENT_KEY = TLS_FIXTURES / "client.key"


def _clear_tls_env(monkeypatch):
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_TLS_CA_CERT_FILE", raising=False)


def test_config_tls_defaults_to_none(monkeypatch):
    _clear_tls_env(monkeypatch)
    config = AdapterConfig.from_env()
    assert config.tls_cert_file is None
    assert config.tls_key_file is None
    assert config.tls_ca_cert_file is None
    assert config.tls_enabled is False
    assert config.mtls_enabled is False


def test_config_tls_reads_env_vars(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE", str(SERVER_CERT))
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE", str(SERVER_KEY))
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_CA_CERT_FILE", str(CA_CERT))

    config = AdapterConfig.from_env()

    assert config.tls_cert_file == str(SERVER_CERT)
    assert config.tls_key_file == str(SERVER_KEY)
    assert config.tls_ca_cert_file == str(CA_CERT)
    assert config.tls_enabled is True
    assert config.mtls_enabled is True


def test_config_tls_enabled_without_ca_is_not_mtls(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE", str(SERVER_CERT))
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE", str(SERVER_KEY))
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_TLS_CA_CERT_FILE", raising=False)

    config = AdapterConfig.from_env()

    assert config.tls_enabled is True
    assert config.mtls_enabled is False


def test_config_rejects_cert_without_key(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE", str(SERVER_CERT))
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE", raising=False)

    with pytest.raises(ValueError, match="must both be provided"):
        AdapterConfig.from_env()


def test_config_rejects_key_without_cert(monkeypatch):
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE", raising=False)
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE", str(SERVER_KEY))

    with pytest.raises(ValueError, match="must both be provided"):
        AdapterConfig.from_env()


def test_build_server_credentials_returns_none_when_tls_disabled(monkeypatch):
    _clear_tls_env(monkeypatch)
    config = AdapterConfig.from_env()
    assert build_server_credentials(config) is None


def test_build_server_credentials_returns_credentials_for_tls(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE", str(SERVER_CERT))
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE", str(SERVER_KEY))
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_TLS_CA_CERT_FILE", raising=False)

    config = AdapterConfig.from_env()
    credentials = build_server_credentials(config)

    assert credentials is not None
    assert isinstance(credentials, grpc.ServerCredentials)


def test_build_server_credentials_returns_mtls_credentials(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_CERT_FILE", str(SERVER_CERT))
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_KEY_FILE", str(SERVER_KEY))
    monkeypatch.setenv("ARIA_UNDERLAY_ADAPTER_TLS_CA_CERT_FILE", str(CA_CERT))

    config = AdapterConfig.from_env()
    credentials = build_server_credentials(config)

    assert credentials is not None
    assert isinstance(credentials, grpc.ServerCredentials)


def _make_config_with_tls(*, mtls: bool = False) -> AdapterConfig:
    return AdapterConfig(
        listen="127.0.0.1:0",
        artifact_dir="/tmp/aria-underlay-adapter/artifacts",
        fake_mode=True,
        fake_profile="confirmed",
        secret_file=None,
        tofu_known_hosts_file="/tmp/aria-underlay-adapter/tofu_known_hosts",
        tls_cert_file=str(SERVER_CERT),
        tls_key_file=str(SERVER_KEY),
        tls_ca_cert_file=str(CA_CERT) if mtls else None,
    )


def _make_insecure_config() -> AdapterConfig:
    return AdapterConfig(
        listen="127.0.0.1:0",
        artifact_dir="/tmp/aria-underlay-adapter/artifacts",
        fake_mode=True,
        fake_profile="confirmed",
        secret_file=None,
        tofu_known_hosts_file="/tmp/aria-underlay-adapter/tofu_known_hosts",
        tls_cert_file=None,
        tls_key_file=None,
        tls_ca_cert_file=None,
    )


def _start_server(config: AdapterConfig) -> tuple[grpc.Server, str]:
    from aria_underlay_adapter.drivers.base import DriverRegistry
    from aria_underlay_adapter.drivers.fake import FakeDriver

    registry = DriverRegistry(default_driver=FakeDriver(profile=config.fake_profile))
    server = build_server(config, registry)
    port = server.add_insecure_port("127.0.0.1:0") if not config.tls_enabled else 0
    # Server is already bound via build_server, so just start it.
    server.start()
    # To get the actual bound port we re-create the server using the returned
    # port from add_insecure_port/add_secure_port.  Since build_server already
    # called one of those with "127.0.0.1:0", the OS assigned a port.
    # grpc.Server does not expose the bound port after start(), so we use
    # a different strategy: call add_*_port *before* start() and capture the
    # port.  We refactor to do that here.
    server.stop(grace=0)
    server.wait_for_termination(timeout=2)

    # Rebuild using explicit port capture.
    server2 = grpc.server(futures.ThreadPoolExecutor(max_workers=4))
    from aria_underlay_adapter.proto import aria_underlay_adapter_pb2_grpc as pb2_grpc
    pb2_grpc.add_UnderlayAdapterServicer_to_server(
        UnderlayAdapterService(registry), server2
    )
    credentials = build_server_credentials(config)
    if credentials is not None:
        assigned_port = server2.add_secure_port("127.0.0.1:0", credentials)
    else:
        assigned_port = server2.add_insecure_port("127.0.0.1:0")
    server2.start()
    return server2, f"127.0.0.1:{assigned_port}"


def test_insecure_server_responds_to_get_capabilities():
    from aria_underlay_adapter.proto import (
        aria_underlay_adapter_pb2 as pb2,
        aria_underlay_adapter_pb2_grpc as pb2_grpc,
    )

    config = _make_insecure_config()
    server, target = _start_server(config)
    try:
        channel = grpc.insecure_channel(target)
        stub = pb2_grpc.UnderlayAdapterStub(channel)
        response = stub.GetCapabilities(
            pb2.GetCapabilitiesRequest(
                context=pb2.RequestContext(
                    request_id="test", trace_id="test", tx_id="", tenant_id="t", site_id="s",
                ),
                device=pb2.DeviceRef(
                    device_id="d1",
                    management_ip="198.51.100.1",
                    management_port=830,
                    vendor_hint=pb2.VENDOR_H3C,
                ),
            )
        )
        assert response.capability is not None
        channel.close()
    finally:
        server.stop(grace=0)


def test_tls_server_rejects_insecure_client():
    config = _make_config_with_tls(mtls=False)
    server, target = _start_server(config)
    try:
        channel = grpc.insecure_channel(target)
        from aria_underlay_adapter.proto import (
            aria_underlay_adapter_pb2 as pb2,
            aria_underlay_adapter_pb2_grpc as pb2_grpc,
        )
        stub = pb2_grpc.UnderlayAdapterStub(channel)
        with pytest.raises(grpc.RpcError):
            stub.GetCapabilities(
                pb2.GetCapabilitiesRequest(
                    context=pb2.RequestContext(
                        request_id="test", trace_id="test", tx_id="", tenant_id="t", site_id="s",
                    ),
                    device=pb2.DeviceRef(
                        device_id="d1",
                        management_ip="198.51.100.1",
                        management_port=830,
                        vendor_hint=pb2.VENDOR_H3C,
                    ),
                ),
                timeout=2,
            )
        channel.close()
    finally:
        server.stop(grace=0)


def test_tls_server_accepts_trusted_client():
    from aria_underlay_adapter.proto import (
        aria_underlay_adapter_pb2 as pb2,
        aria_underlay_adapter_pb2_grpc as pb2_grpc,
    )

    config = _make_config_with_tls(mtls=False)
    server, target = _start_server(config)
    try:
        ca_cert_pem = CA_CERT.read_bytes()
        channel_creds = grpc.ssl_channel_credentials(root_certificates=ca_cert_pem)
        channel = grpc.secure_channel(target, channel_creds)
        stub = pb2_grpc.UnderlayAdapterStub(channel)
        response = stub.GetCapabilities(
            pb2.GetCapabilitiesRequest(
                context=pb2.RequestContext(
                    request_id="test", trace_id="test", tx_id="", tenant_id="t", site_id="s",
                ),
                device=pb2.DeviceRef(
                    device_id="d1",
                    management_ip="198.51.100.1",
                    management_port=830,
                    vendor_hint=pb2.VENDOR_H3C,
                ),
            ),
            timeout=5,
        )
        assert response.capability is not None
        channel.close()
    finally:
        server.stop(grace=0)


def test_mtls_server_rejects_client_without_cert():
    config = _make_config_with_tls(mtls=True)
    server, target = _start_server(config)
    try:
        ca_cert_pem = CA_CERT.read_bytes()
        channel_creds = grpc.ssl_channel_credentials(root_certificates=ca_cert_pem)
        channel = grpc.secure_channel(target, channel_creds)
        from aria_underlay_adapter.proto import (
            aria_underlay_adapter_pb2 as pb2,
            aria_underlay_adapter_pb2_grpc as pb2_grpc,
        )
        stub = pb2_grpc.UnderlayAdapterStub(channel)
        with pytest.raises(grpc.RpcError):
            stub.GetCapabilities(
                pb2.GetCapabilitiesRequest(
                    context=pb2.RequestContext(
                        request_id="test", trace_id="test", tx_id="", tenant_id="t", site_id="s",
                    ),
                    device=pb2.DeviceRef(
                        device_id="d1",
                        management_ip="198.51.100.1",
                        management_port=830,
                        vendor_hint=pb2.VENDOR_H3C,
                    ),
                ),
                timeout=2,
            )
        channel.close()
    finally:
        server.stop(grace=0)


def test_mtls_server_accepts_client_with_cert():
    from aria_underlay_adapter.proto import (
        aria_underlay_adapter_pb2 as pb2,
        aria_underlay_adapter_pb2_grpc as pb2_grpc,
    )

    config = _make_config_with_tls(mtls=True)
    server, target = _start_server(config)
    try:
        ca_cert_pem = CA_CERT.read_bytes()
        client_cert_pem = CLIENT_CERT.read_bytes()
        client_key_pem = CLIENT_KEY.read_bytes()
        channel_creds = grpc.ssl_channel_credentials(
            root_certificates=ca_cert_pem,
            private_key=client_key_pem,
            certificate_chain=client_cert_pem,
        )
        channel = grpc.secure_channel(target, channel_creds)
        stub = pb2_grpc.UnderlayAdapterStub(channel)
        response = stub.GetCapabilities(
            pb2.GetCapabilitiesRequest(
                context=pb2.RequestContext(
                    request_id="test", trace_id="test", tx_id="", tenant_id="t", site_id="s",
                ),
                device=pb2.DeviceRef(
                    device_id="d1",
                    management_ip="198.51.100.1",
                    management_port=830,
                    vendor_hint=pb2.VENDOR_H3C,
                ),
            ),
            timeout=5,
        )
        assert response.capability is not None
        channel.close()
    finally:
        server.stop(grace=0)
