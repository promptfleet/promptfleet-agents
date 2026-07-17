from __future__ import annotations

import base64
import json
import threading
import time
import uuid
from dataclasses import dataclass
from typing import Callable, Mapping, Protocol, Sequence
from urllib import error, parse, request

TOKEN_EXCHANGE_GRANT = "urn:ietf:params:oauth:grant-type:token-exchange"
JWT_BEARER_GRANT = "urn:ietf:params:oauth:grant-type:jwt-bearer"
ACCESS_TOKEN_TYPE = "urn:ietf:params:oauth:token-type:access_token"
PF_INVOKE_TOKEN_TYPE = "urn:promptfleet:params:oauth:token-type:pf-invoke-jwt"


class JwtSigner(Protocol):
    @property
    def key_id(self) -> str: ...

    def sign(self, signing_input: bytes) -> bytes: ...


class HttpTransport(Protocol):
    def post_form(self, url: str, form: Mapping[str, str]) -> tuple[int, Mapping[str, object]]: ...


@dataclass(frozen=True)
class ServiceAccountClientOptions:
    client_id: str
    oauth_token_url: str
    oauth_audience: str
    invoke_token_url: str
    signer: JwtSigner
    oauth_scopes: Sequence[str]
    transport: HttpTransport | None = None
    clock: Callable[[], float] = time.time


@dataclass(frozen=True)
class InvokeTokenRequest:
    audience: str
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
        if not options.client_id or not options.oauth_token_url or not options.oauth_audience or not options.invoke_token_url:
            raise ValueError("client_id, oauth_token_url, oauth_audience, and invoke_token_url are required")
        if not options.oauth_scopes:
            raise ValueError("oauth_scopes must include the PromptFleet API audience scope")
        self._options = options
        self._transport = options.transport or UrllibTransport()
        self._oauth: AccessToken | None = None
        self._invoke: dict[tuple[str, str, tuple[str, ...]], AccessToken] = {}
        self._lock = threading.Lock()

    def get_oauth_access_token(self) -> AccessToken:
        with self._lock:
            if self._fresh(self._oauth):
                return self._oauth
            self._oauth = self._request_oauth_access_token()
            return self._oauth

    def get_invoke_token(self, requested: InvokeTokenRequest) -> AccessToken:
        if not requested.audience or not requested.resource or not requested.scopes:
            raise ValueError("audience, resource, and at least one scope are required")
        cache_key = (requested.audience, requested.resource, tuple(sorted(requested.scopes)))
        with self._lock:
            cached = self._invoke.get(cache_key)
            if self._fresh(cached):
                return cached
            source = self._oauth if self._fresh(self._oauth) else self._request_oauth_access_token()
            self._oauth = source
            token = self._post_token(
                self._options.invoke_token_url,
                {
                    "grant_type": TOKEN_EXCHANGE_GRANT,
                    "subject_token_type": ACCESS_TOKEN_TYPE,
                    "subject_token": source.access_token,
                    "audience": requested.audience,
                    "resource": requested.resource,
                    "scope": " ".join(requested.scopes),
                    "requested_token_type": PF_INVOKE_TOKEN_TYPE,
                },
                "PromptFleet invoke token",
            )
            self._invoke[cache_key] = token
            return token

    def authorization_header(self, requested: InvokeTokenRequest) -> str:
        return f"Bearer {self.get_invoke_token(requested).access_token}"

    def _request_oauth_access_token(self) -> AccessToken:
        now = int(self._options.clock())
        header = _encode_json({"alg": "RS256", "typ": "JWT", "kid": self._options.signer.key_id})
        claims = _encode_json(
            {
                "iss": self._options.client_id,
                "sub": self._options.client_id,
                "aud": self._options.oauth_audience,
                "iat": now,
                "exp": now + 60,
                "jti": str(uuid.uuid4()),
            }
        )
        signing_input = f"{header}.{claims}".encode("ascii")
        assertion = f"{signing_input.decode('ascii')}.{_base64url(self._options.signer.sign(signing_input))}"
        form = {
            "grant_type": JWT_BEARER_GRANT,
            "assertion": assertion,
        }
        form["scope"] = " ".join(dict.fromkeys(("openid", *self._options.oauth_scopes)))
        token = self._post_token(self._options.oauth_token_url, form, "OAuth token")
        return AccessToken(
            access_token=token.access_token,
            token_type=token.token_type,
            expires_at=min(token.expires_at, self._options.clock() + 300),
            scope=token.scope,
        )

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


class UrllibTransport:
    def post_form(self, url: str, form: Mapping[str, str]) -> tuple[int, Mapping[str, object]]:
        body = parse.urlencode(form).encode("utf-8")
        outgoing = request.Request(
            url,
            data=body,
            method="POST",
            headers={"content-type": "application/x-www-form-urlencoded"},
        )
        try:
            with request.urlopen(outgoing, timeout=30) as response:
                return response.status, json.loads(response.read())
        except error.HTTPError as exc:
            return exc.code, json.loads(exc.read())


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
