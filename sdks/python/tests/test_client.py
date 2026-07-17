import ssl

from promptfleet_auth import (
    InvokeTokenRequest,
    PromptFleetServiceAccountClient,
    ServiceAccountClientOptions,
)
from promptfleet_auth.client import UrllibTransport


class FakeSigner:
    key_id = "provider-key"

    def sign(self, signing_input: bytes) -> bytes:
        assert signing_input.count(b".") == 1
        return b"signature"


class FakeTransport:
    def __init__(self) -> None:
        self.requests: list[tuple[str, dict[str, str]]] = []

    def post_form(self, url: str, form: dict[str, str]):
        self.requests.append((url, dict(form)))
        if url.endswith("/oauth/v2/token"):
            return 200, {"access_token": "oauth-token", "token_type": "Bearer", "expires_in": 300}
        return 200, {"access_token": "invoke-token", "token_type": "Bearer", "expires_in": 300}


def test_exchanges_and_caches_resource_bound_invoke_token() -> None:
    transport = FakeTransport()
    client = PromptFleetServiceAccountClient(
        ServiceAccountClientOptions(
            client_id="machine-user-id",
            oauth_token_url="https://identity.example/oauth/v2/token",
            oauth_audience="https://identity.example",
            oauth_scopes=["urn:zitadel:iam:org:project:id:promptfleet:aud"],
            invoke_token_url="https://api.example/v1/invoke/token",
            signer=FakeSigner(),
            transport=transport,
            clock=lambda: 1000.0,
        )
    )
    requested = InvokeTokenRequest("pf-workload-api", "workload:wld-1", ["workload.invoke"])

    assert client.get_invoke_token(requested).access_token == "invoke-token"
    assert client.get_invoke_token(requested).access_token == "invoke-token"
    assert len(transport.requests) == 2
    assert transport.requests[0][1]["grant_type"] == "urn:ietf:params:oauth:grant-type:jwt-bearer"
    assert transport.requests[0][1]["assertion"]
    assert "openid" in transport.requests[0][1]["scope"].split()
    assert transport.requests[1][1]["subject_token"] == "oauth-token"
    assert transport.requests[1][1]["resource"] == "workload:wld-1"


def test_rejects_missing_resource_binding() -> None:
    client = PromptFleetServiceAccountClient(
        ServiceAccountClientOptions(
            client_id="machine-user-id",
            oauth_token_url="https://identity.example/oauth/v2/token",
            oauth_audience="https://identity.example",
            oauth_scopes=["urn:zitadel:iam:org:project:id:promptfleet:aud"],
            invoke_token_url="https://api.example/v1/invoke/token",
            signer=FakeSigner(),
            transport=FakeTransport(),
        )
    )
    try:
        client.get_invoke_token(InvokeTokenRequest("pf-workload-api", "", ["workload.invoke"]))
    except ValueError as exc:
        assert "resource" in str(exc)
    else:
        raise AssertionError("missing resource must be rejected")


def test_default_http_transport_requires_hostname_and_certificate_verification() -> None:
    context = UrllibTransport()._ssl_context

    assert context.check_hostname is True
    assert context.verify_mode == ssl.CERT_REQUIRED
