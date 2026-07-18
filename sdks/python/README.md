# PromptFleet service-account auth for Python

Python 3.11+ client for confidential applications. Implement `JwtSigner` with a KMS, HSM, TPM, or secret-manager key; the private RSA key never needs to enter PromptFleet.

After PromptFleet activates the first registered public key, use the service account's displayed OAuth client ID as `client_id`.

```python
client = PromptFleetServiceAccountClient(
    ServiceAccountClientOptions(
        client_id="svc:example",
        signer=kms_signer,
    )
)

token = client.get_invoke_token(
    InvokeTokenRequest("https://api.promptfleet.ai/workloads", ["workload.invoke"])
)
```

`PemRsaSigner` is available through the `pem` extra for development and protected-filesystem deployments. Prefer a non-exportable KMS/HSM signer in production.

Async frameworks can use `AsyncPromptFleetServiceAccountClient` with either a
normal signer or an `AsyncJwtSigner` backed by a KMS call. Token refresh is
single-flight per resource, and `authorization_headers()` composes directly
with standard `httpx`, A2A, or AG-UI clients without exposing a bearer token to
browser code.

The client uses OAuth `client_credentials` with RFC 7523 `private_key_jwt`. The assertion audience is the PromptFleet token endpoint; the returned five-minute access token is bound to the exact protected-resource URI. No identity-provider-specific project scope is exposed.

The default HTTP transport verifies TLS hostnames and certificate chains using
the operating system trust store through PyCA `truststore`, including managed
enterprise roots. Applications that need an isolated CA policy can pass an
explicitly configured `ssl.SSLContext` to `UrllibTransport`.

## Runnable Agent Edge example

[`examples/invoke_agent.py`](examples/invoke_agent.py) performs an authenticated
`GetAgentCard` request without printing any token or key material. Install the
local package with its protected-filesystem signer and configure:

```bash
python -m pip install -e '.[pem]'
export PF_SERVICE_ACCOUNT_CLIENT_ID='...'
export PF_SERVICE_ACCOUNT_KEY_ID='...'
export PF_SERVICE_ACCOUNT_PRIVATE_KEY_FILE='/protected/path/private-key.pem'
export PF_AGENT_EDGE_URL='https://my-agent.edge.promptfleet.ai/jsonrpc'
export PF_AGENT_RESOURCE='https://my-agent.edge.promptfleet.ai'
python examples/invoke_agent.py
```
