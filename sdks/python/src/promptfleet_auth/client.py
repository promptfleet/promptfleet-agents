from __future__ import annotations

import base64
import asyncio
import inspect
import json
import ssl
import threading
import time
import uuid
from dataclasses import dataclass
from typing import Awaitable, Callable, Mapping, Protocol, Sequence
from urllib import error, parse, request

import truststore

CLIENT_ASSERTION_TYPE = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
DEFAULT_INVOKE_TRUST_TOKEN_URL = "https://issuer.promptfleet.ai/invoke-trust/token"


class JwtSigner(Protocol):
    @property
    def key_id(self) -> str: ...

    def sign(self, signing_input: bytes) -> bytes: ...


class HttpTransport(Protocol):
    def post_form(self, url: str, form: Mapping[str, str]) -> tuple[int, Mapping[str, object]]: ...


class AsyncJwtSigner(Protocol):
    @property
    def key_id(self) -> str: ...

    def sign(self, signing_input: bytes) -> Awaitable[bytes]: ...


class AsyncHttpTransport(Protocol):
    def post_form(
        self, url: str, form: Mapping[str, str]
    ) -> Awaitable[tuple[int, Mapping[str, object]]]: ...


@dataclass(frozen=True)
class ServiceAccountClientOptions:
    client_id: str
    signer: JwtSigner
    token_url: str = DEFAULT_INVOKE_TRUST_TOKEN_URL
    transport: HttpTransport | None = None
    clock: Callable[[], float] = time.time


@dataclass(frozen=True)
class AsyncServiceAccountClientOptions:
    client_id: str
    signer: JwtSigner | AsyncJwtSigner
    token_url: str = DEFAULT_INVOKE_TRUST_TOKEN_URL
    transport: AsyncHttpTransport | None = None
    clock: Callable[[], float] = time.time


@dataclass(frozen=True)
class InvokeTokenRequest:
    resource: str
    scopes: Sequence[str]


@dataclass(frozen=True)
class AccessToken:
    access_token: str
    token_type: str
    expires_at: float
    scope: str | None = None


class PromptFleetServiceAccountClient:
    def __init__(self, options: ServiceAccountClientOptions) -> None:
        if not options.client_id or not options.token_url or not options.signer.key_id:
            raise ValueError("client_id, token_url, and a signer with key_id are required")
        self._options = options
        self._transport = options.transport or UrllibTransport()
        self._invoke: dict[tuple[str, tuple[str, ...]], AccessToken] = {}
        self._lock = threading.Lock()

    def get_invoke_token(self, requested: InvokeTokenRequest) -> AccessToken:
        if not requested.resource or not requested.scopes:
            raise ValueError("resource and at least one scope are required")
        cache_key = (requested.resource, tuple(sorted(requested.scopes)))
        with self._lock:
            cached = self._invoke.get(cache_key)
            if self._fresh(cached):
                return cached
            assertion = self._create_client_assertion()
            token = self._post_token(
                self._options.token_url,
                {
                    "grant_type": "client_credentials",
                    "client_id": self._options.client_id,
                    "client_assertion_type": CLIENT_ASSERTION_TYPE,
                    "client_assertion": assertion,
                    "resource": requested.resource,
                    "scope": " ".join(requested.scopes),
                },
                "PromptFleet invoke token",
            )
            self._invoke[cache_key] = token
            return token

    def authorization_header(self, requested: InvokeTokenRequest) -> str:
        return f"Bearer {self.get_invoke_token(requested).access_token}"

    def _create_client_assertion(self) -> str:
        now = int(self._options.clock())
        header = _encode_json({"alg": "RS256", "typ": "JWT", "kid": self._options.signer.key_id})
        claims = _encode_json(
            {
                "iss": self._options.client_id,
                "sub": self._options.client_id,
                "aud": self._options.token_url,
                "iat": now,
                "exp": now + 60,
                "jti": str(uuid.uuid4()),
            }
        )
        signing_input = f"{header}.{claims}".encode("ascii")
        return f"{signing_input.decode('ascii')}.{_base64url(self._options.signer.sign(signing_input))}"

    def _post_token(self, url: str, form: Mapping[str, str], label: str) -> AccessToken:
        status, body = self._transport.post_form(url, form)
        if status < 200 or status >= 300:
            raise RuntimeError(f"{label} request failed ({status}): {body.get('error', 'unknown error')}")
        access_token = body.get("access_token")
        expires_in = body.get("expires_in")
        if not isinstance(access_token, str) or not isinstance(expires_in, (int, float)):
            raise RuntimeError(f"{label} response is malformed")
        scope = body.get("scope")
        return AccessToken(
            access_token=access_token,
            token_type=body.get("token_type") if isinstance(body.get("token_type"), str) else "Bearer",
            expires_at=self._options.clock() + float(expires_in),
            scope=scope if isinstance(scope, str) else None,
        )

    def _fresh(self, token: AccessToken | None) -> bool:
        return token is not None and token.expires_at - self._options.clock() > 30


class AsyncPromptFleetServiceAccountClient:
    """Async service-account client for web frameworks and cloud KMS signers."""

    def __init__(self, options: AsyncServiceAccountClientOptions) -> None:
        if (
            not options.client_id
            or not options.token_url
            or not options.signer.key_id
        ):
            raise ValueError(
                "client_id, token_url, and a signer with key_id are required"
            )
        self._options = options
        self._transport = options.transport or AsyncUrllibTransport()
        self._invoke: dict[tuple[str, tuple[str, ...]], AccessToken] = {}
        self._invoke_locks: dict[tuple[str, tuple[str, ...]], asyncio.Lock] = {}

    async def get_invoke_token(self, requested: InvokeTokenRequest) -> AccessToken:
        if not requested.resource or not requested.scopes:
            raise ValueError("resource and at least one scope are required")
        cache_key = (
            requested.resource,
            tuple(sorted(requested.scopes)),
        )
        cached = self._invoke.get(cache_key)
        if self._fresh(cached):
            return cached
        lock = self._invoke_locks.setdefault(cache_key, asyncio.Lock())
        async with lock:
            cached = self._invoke.get(cache_key)
            if self._fresh(cached):
                return cached
            assertion = await self._create_client_assertion()
            token = await self._post_token(
                self._options.token_url,
                {
                    "grant_type": "client_credentials",
                    "client_id": self._options.client_id,
                    "client_assertion_type": CLIENT_ASSERTION_TYPE,
                    "client_assertion": assertion,
                    "resource": requested.resource,
                    "scope": " ".join(requested.scopes),
                },
                "PromptFleet invoke token",
            )
            self._invoke[cache_key] = token
            return token

    async def authorization_headers(
        self,
        requested: InvokeTokenRequest,
        headers: Mapping[str, str] | None = None,
    ) -> dict[str, str]:
        token = await self.get_invoke_token(requested)
        authenticated = dict(headers or {})
        authenticated["authorization"] = f"Bearer {token.access_token}"
        return authenticated

    async def _create_client_assertion(self) -> str:
        now = int(self._options.clock())
        header = _encode_json(
            {"alg": "RS256", "typ": "JWT", "kid": self._options.signer.key_id}
        )
        claims = _encode_json(
            {
                "iss": self._options.client_id,
                "sub": self._options.client_id,
                "aud": self._options.token_url,
                "iat": now,
                "exp": now + 60,
                "jti": str(uuid.uuid4()),
            }
        )
        signing_input = f"{header}.{claims}".encode("ascii")
        signature = self._options.signer.sign(signing_input)
        if inspect.isawaitable(signature):
            signature = await signature
        return (
            f"{signing_input.decode('ascii')}.{_base64url(signature)}"
        )

    async def _post_token(
        self, url: str, form: Mapping[str, str], label: str
    ) -> AccessToken:
        status, body = await self._transport.post_form(url, form)
        if status < 200 or status >= 300:
            raise RuntimeError(
                f"{label} request failed ({status}): {body.get('error', 'unknown error')}"
            )
        access_token = body.get("access_token")
        expires_in = body.get("expires_in")
        if not isinstance(access_token, str) or not isinstance(
            expires_in, (int, float)
        ):
            raise RuntimeError(f"{label} response is malformed")
        scope = body.get("scope")
        return AccessToken(
            access_token=access_token,
            token_type=(
                body.get("token_type")
                if isinstance(body.get("token_type"), str)
                else "Bearer"
            ),
            expires_at=self._options.clock() + float(expires_in),
            scope=scope if isinstance(scope, str) else None,
        )

    def _fresh(self, token: AccessToken | None) -> bool:
        return token is not None and token.expires_at - self._options.clock() > 30


class UrllibTransport:
    def __init__(self, ssl_context: ssl.SSLContext | None = None) -> None:
        self._ssl_context = ssl_context or truststore.SSLContext(ssl.PROTOCOL_TLS_CLIENT)

    def post_form(self, url: str, form: Mapping[str, str]) -> tuple[int, Mapping[str, object]]:
        body = parse.urlencode(form).encode("utf-8")
        outgoing = request.Request(
            url,
            data=body,
            method="POST",
            headers={"content-type": "application/x-www-form-urlencoded"},
        )
        try:
            with request.urlopen(outgoing, timeout=30, context=self._ssl_context) as response:
                return response.status, json.loads(response.read())
        except error.HTTPError as exc:
            return exc.code, json.loads(exc.read())


class AsyncUrllibTransport:
    """Async adapter over the verified stdlib transport using a worker thread."""

    def __init__(self, ssl_context: ssl.SSLContext | None = None) -> None:
        self._transport = UrllibTransport(ssl_context)

    async def post_form(
        self, url: str, form: Mapping[str, str]
    ) -> tuple[int, Mapping[str, object]]:
        return await asyncio.to_thread(self._transport.post_form, url, form)


class PemRsaSigner:
    def __init__(self, key_id: str, private_key_pem: bytes, password: bytes | None = None) -> None:
        try:
            from cryptography.hazmat.primitives.serialization import load_pem_private_key
        except ImportError as exc:
            raise RuntimeError("install promptfleet-service-account-auth[pem]") from exc
        self._key_id = key_id
        self._key = load_pem_private_key(private_key_pem, password=password)

    @property
    def key_id(self) -> str:
        return self._key_id

    def sign(self, signing_input: bytes) -> bytes:
        from cryptography.hazmat.primitives import hashes
        from cryptography.hazmat.primitives.asymmetric import padding

        return self._key.sign(signing_input, padding.PKCS1v15(), hashes.SHA256())


def _encode_json(value: object) -> str:
    return _base64url(json.dumps(value, separators=(",", ":")).encode("utf-8"))


def _base64url(value: bytes) -> str:
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")
