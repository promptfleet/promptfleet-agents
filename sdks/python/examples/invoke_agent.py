from __future__ import annotations

import json
import os
import ssl
import uuid
from pathlib import Path
from urllib import error, request

import truststore

from promptfleet_auth import (
    InvokeTokenRequest,
    PemRsaSigner,
    PromptFleetServiceAccountClient,
    ServiceAccountClientOptions,
)


def required(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise RuntimeError(f"{name} is required")
    return value


client = PromptFleetServiceAccountClient(
    ServiceAccountClientOptions(
        client_id=required("PF_SERVICE_ACCOUNT_CLIENT_ID"),
        token_url=os.environ.get(
            "PF_OAUTH_TOKEN_URL", "https://issuer.promptfleet.ai/invoke-trust/token"
        ),
        signer=PemRsaSigner(
            required("PF_SERVICE_ACCOUNT_KEY_ID"),
            Path(required("PF_SERVICE_ACCOUNT_PRIVATE_KEY_FILE")).read_bytes(),
        ),
    )
)

invoke_request = InvokeTokenRequest(
    resource=required("PF_AGENT_RESOURCE"),
    scopes=["agent.invoke"],
)
payload = json.dumps(
    {
        "jsonrpc": "2.0",
        "id": f"service-account-example-{uuid.uuid4()}",
        "method": "GetAgentCard",
        "params": None,
    }
).encode("utf-8")
outgoing = request.Request(
    required("PF_AGENT_EDGE_URL"),
    data=payload,
    method="POST",
    headers={
        "authorization": client.authorization_header(invoke_request),
        "content-type": "application/json",
    },
)

try:
    ssl_context = truststore.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    with request.urlopen(outgoing, timeout=30, context=ssl_context) as response:
        body = response.read().decode("utf-8")
except error.HTTPError as exc:
    body = exc.read().decode("utf-8")
    raise RuntimeError(f"Agent Edge request failed ({exc.code}): {body}") from exc

result = json.loads(body)
if result.get("error") or not isinstance(result.get("result", {}).get("name"), str):
    raise RuntimeError(f"Agent Edge returned an invalid AgentCard response: {body}")

print(f"Authenticated AgentCard: {result['result']['name']}")
