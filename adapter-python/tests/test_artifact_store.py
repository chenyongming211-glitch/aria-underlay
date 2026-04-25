from aria_underlay_adapter.artifact_store import ArtifactStore


def test_save_json(tmp_path):
    store = ArtifactStore(str(tmp_path))
    path = store.save_json("leaf-a", "tx-1", "rollback.json", {"ok": True})

    assert path.exists()
    assert "leaf-a" in str(path)
    assert "tx-1" in str(path)

