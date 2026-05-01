from aria_underlay_adapter.config import AdapterConfig


def test_config_from_env_defaults(monkeypatch):
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_LISTEN", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ARTIFACT_DIR", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_FAKE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_FAKE_PROFILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_SECRET_FILE", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_TOFU_KNOWN_HOSTS_FILE", raising=False)

    config = AdapterConfig.from_env()

    assert config.listen == "127.0.0.1:50051"
    assert config.fake_mode is True
    assert config.fake_profile == "confirmed"
    assert config.secret_file is None
    assert config.tofu_known_hosts_file == "/tmp/aria-underlay-adapter/tofu_known_hosts"


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
