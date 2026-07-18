# PromptFleet service-account auth for TypeScript

Node.js 20+ client for confidential applications. The private RSA key remains in the application’s KMS, HSM, TPM, secret manager, or protected filesystem; PromptFleet receives only the public key during credential registration.

Provide a `JwtSigner` backed by your key system, then request short-lived, resource-bound invoke tokens directly from PromptFleet Invoke Trust:

After PromptFleet activates the first registered public key, copy the service account's displayed OAuth client ID into `PF_SERVICE_ACCOUNT_CLIENT_ID`.

```ts
const client = new PromptFleetServiceAccountClient({
  clientId: process.env.PF_SERVICE_ACCOUNT_CLIENT_ID!,
  signer: kmsSigner,
});

const token = await client.getInvokeToken({
  resource: "https://api.promptfleet.ai/workloads",
  scopes: ["workload.invoke"],
});
```

`createNodePrivateKeySigner` is provided for development or protected-filesystem deployments. Prefer a non-exportable KMS/HSM signer in production.

The client uses OAuth `client_credentials` with RFC 7523 `private_key_jwt`. The assertion audience is the PromptFleet token endpoint; the returned five-minute access token is bound to the exact protected-resource URI. No identity-provider-specific project scope is exposed.

## Runnable Agent Edge example

[`examples/invoke-agent.ts`](examples/invoke-agent.ts) performs an authenticated
`GetAgentCard` request without printing any token or key material. Configure:

```bash
export PF_SERVICE_ACCOUNT_CLIENT_ID='...'
export PF_SERVICE_ACCOUNT_KEY_ID='...'
export PF_SERVICE_ACCOUNT_PRIVATE_KEY_FILE='/protected/path/private-key.pem'
export PF_AGENT_EDGE_URL='https://my-agent.edge.promptfleet.ai/jsonrpc'
export PF_AGENT_RESOURCE='https://my-agent.edge.promptfleet.ai'
npm run example:agent
```
