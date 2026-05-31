from aria_underlay_adapter.config import AdapterConfig


def test_config_from_env_defaults(monkeypatch):
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_LISTEN", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ARTIFACT_DIR", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_FAKE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_FAKE_PROFILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_SECRET_FILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_TOFU_KNOWN_HOSTS_FILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_GNMI_PROBE_ENABLED", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_GNMI_PORT", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_GNMI_TLS_ENABLED", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_GNMI_TLS_CA_CERT_FILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_GNMI_TLS_CERT_FILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_GNMI_TLS_KEY_FILE", raising=False)

    config = AdapterConfig.from_env()

    assert config.listen == "127.0.0.1:50051"
    assert config.fake_mode is True
    assert config.fake_profile == "confirmed"
    assert config.secret_file is None
    assert config.tofu_known_hosts_file == "/tmp/aria-underlay-adapter/tofu_known_hosts"
    assert config.gnmi_probe_enabled is False
    assert config.gnmi_port == 57400
    assert config.gnmi_tls_enabled is True


def test_config_reads_fake_profile(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_FAKE_PROFILE", "candidate_only")

    config = AdapterConfig.from_env()

    assert config.fake_profile == "candidate_only"


def test_config_reads_secret_file(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_SECRET_FILE", "/tmp/aria-underlay-secrets.json")

    config = AdapterConfig.from_env()

    assert config.secret_file == "/tmp/aria-underlay-secrets.json"


def test_config_reads_tofu_known_hosts_file(monkeypatch):
    monkeypatch.setenv(
        "ARIA_UNDERLAY_TOFU_KNOWN_HOSTS_FILE",
        "/var/lib/aria-underlay/tofu_known_hosts",
    )

    config = AdapterConfig.from_env()

    assert config.tofu_known_hosts_file == "/var/lib/aria-underlay/tofu_known_hosts"


def test_config_reads_gnmi_probe_settings(monkeypatch):
    monkeypatch.setenv("ARIA_UNDERLAY_GNMI_PROBE_ENABLED", "1")
    monkeypatch.setenv("ARIA_UNDERLAY_GNMI_PORT", "57401")
    monkeypatch.setenv("ARIA_UNDERLAY_GNMI_TLS_ENABLED", "0")
    monkeypatch.setenv("ARIA_UNDERLAY_GNMI_TLS_CA_CERT_FILE", "/etc/aria/gnmi-ca.pem")
    monkeypatch.setenv("ARIA_UNDERLAY_GNMI_TLS_CERT_FILE", "/etc/aria/gnmi-client.pem")
    monkeypatch.setenv("ARIA_UNDERLAY_GNMI_TLS_KEY_FILE", "/etc/aria/gnmi-client.key")

    config = AdapterConfig.from_env()

    assert config.gnmi_probe_enabled is True
    assert config.gnmi_port == 57401
    assert config.gnmi_tls_enabled is False
    assert config.gnmi_tls_ca_cert_file == "/etc/aria/gnmi-ca.pem"
    assert config.gnmi_tls_cert_file == "/etc/aria/gnmi-client.pem"
    assert config.gnmi_tls_key_file == "/etc/aria/gnmi-client.key"
