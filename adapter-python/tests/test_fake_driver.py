from aria_underlay_adapter.drivers.fake import FakeDriver


def test_fake_driver_confirmed_profile_returns_confirmed_commit():
    response = FakeDriver(profile="confirmed").get_capabilities(request=None)

    assert response.capability.supports_candidate is True
    assert response.capability.supports_confirmed_commit is True
    assert not response.errors


def test_fake_driver_auth_failed_profile_returns_adapter_error():
    response = FakeDriver(profile="auth_failed").get_capabilities(request=None)

    assert response.errors
    assert response.errors[0].code == "AUTH_FAILED"

