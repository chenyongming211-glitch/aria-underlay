from aria_underlay_adapter.config import AdapterConfig


def test_config_from_env_defaults(monkeypatch):
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_LISTEN", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ARTIFACT_DIR", raising=False)
    monkeypatch.delenv("ARIA_UNDERLAY_ADAPTER_FAKE", raising=False)

    config = AdapterConfig.from_env()

    assert config.listen == "127.0.0.1:50051"
    assert config.fake_mode is True

