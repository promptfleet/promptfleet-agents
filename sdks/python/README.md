# PromptFleet service-account auth for Python

Python 3.12+ client for confidential applications. Implement `JwtSigner` with a KMS, HSM, TPM, or secret-manager key; the private RSA key never needs to enter PromptFleet.

After PromptFleet activates the first registered public key, use the service account's displayed OAuth client ID as `client_id`.

```python
client = PromptFleetServiceAccountClient(
    ServiceAccountClientOptions(
        client_id="machine-user-id",
        oauth_token_url="https://identity.example/oauth/v2/token",
        oauth_audience="https://identity.example",
        oauth_scopes=["urn:zitadel:iam:org:project:id:YOUR_PROMPTFLEET_PROJECT_ID:aud"],
        invoke_token_url="https://api.promptfleet.ai/v1/invoke/token",
        signer=kms_signer,
    )
)

token = client.get_invoke_token(
    InvokeTokenRequest("pf-workload-api", "workload:wld-123", ["workload.invoke"])
)
```

`PemRsaSigner` is available through the `pem` extra for development and protected-filesystem deployments. Prefer a non-exportable KMS/HSM signer in production.

`oauth_audience` is the ZITADEL custom-domain origin used in the signed assertion, not the token endpoint URL. `oauth_scopes` must include the PromptFleet API project-audience scope shown by your platform administrator. The client uses the RFC 7523 JWT-bearer grant, adds the required `openid` scope, and refreshes the source token within five minutes even if ZITADEL reports a longer lifetime.

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
export PF_OAUTH_PROJECT_SCOPE='urn:zitadel:iam:org:project:id:...:aud'
export PF_AGENT_EDGE_URL='https://my-agent.edge.promptfleet.ai/jsonrpc'
export PF_AGENT_RESOURCE='agent:A-...'
python examples/invoke_agent.py
```
