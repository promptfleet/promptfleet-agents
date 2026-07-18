import asyncio
import ssl

from promptfleet_auth import (
    AsyncPromptFleetServiceAccountClient,
    AsyncServiceAccountClientOptions,
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
        return 200, {"access_token": "invoke-token", "token_type": "Bearer", "expires_in": 300}


def test_exchanges_and_caches_resource_bound_invoke_token() -> None:
    transport = FakeTransport()
    client = PromptFleetServiceAccountClient(
        ServiceAccountClientOptions(
            client_id="svc:test",
            token_url="https://issuer.promptfleet.ai/invoke-trust/token",
            signer=FakeSigner(),
            transport=transport,
            clock=lambda: 1000.0,
        )
    )
    requested = InvokeTokenRequest("https://api.promptfleet.ai/workloads", ["workload.invoke"])

    assert client.get_invoke_token(requested).access_token == "invoke-token"
    assert client.get_invoke_token(requested).access_token == "invoke-token"
    assert len(transport.requests) == 1
    assert transport.requests[0][1]["grant_type"] == "client_credentials"
    assert transport.requests[0][1]["client_id"] == "svc:test"
    assert transport.requests[0][1]["client_assertion"]
    assert transport.requests[0][1]["resource"] == "https://api.promptfleet.ai/workloads"


def test_rejects_missing_resource_binding() -> None:
    client = PromptFleetServiceAccountClient(
        ServiceAccountClientOptions(
            client_id="svc:test",
            signer=FakeSigner(),
            transport=FakeTransport(),
        )
    )
    try:
        client.get_invoke_token(InvokeTokenRequest("", ["workload.invoke"]))
    except ValueError as exc:
        assert "resource" in str(exc)
    else:
        raise AssertionError("missing resource must be rejected")


def test_default_http_transport_requires_hostname_and_certificate_verification() -> None:
    context = UrllibTransport()._ssl_context

    assert context.check_hostname is True
    assert context.verify_mode == ssl.CERT_REQUIRED


def test_async_client_single_flights_concurrent_token_requests() -> None:
    class AsyncSigner:
        key_id = "provider-key"

        async def sign(self, signing_input: bytes) -> bytes:
            assert signing_input.count(b".") == 1
            return b"signature"

    class AsyncTransport:
        def __init__(self) -> None:
            self.requests: list[tuple[str, dict[str, str]]] = []

        async def post_form(self, url: str, form: dict[str, str]):
            self.requests.append((url, dict(form)))
            await asyncio.sleep(0)
            return 200, {
                "access_token": "invoke-token",
                "token_type": "Bearer",
                "expires_in": 300,
            }

    async def exercise() -> None:
        transport = AsyncTransport()
        client = AsyncPromptFleetServiceAccountClient(
            AsyncServiceAccountClientOptions(
                client_id="svc:test",
                signer=AsyncSigner(),
                transport=transport,
                clock=lambda: 1000.0,
            )
        )
        requested = InvokeTokenRequest("https://demo.edge.promptfleet.ai", ["agent.invoke"])

        tokens = await asyncio.gather(
            client.get_invoke_token(requested),
            client.get_invoke_token(requested),
        )
        headers = await client.authorization_headers(
            requested, {"x-client": "example"}
        )

        assert [token.access_token for token in tokens] == [
            "invoke-token",
            "invoke-token",
        ]
        assert headers == {
            "x-client": "example",
            "authorization": "Bearer invoke-token",
        }
        assert len(transport.requests) == 1

    asyncio.run(exercise())
