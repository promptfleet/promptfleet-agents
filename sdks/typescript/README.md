# PromptFleet service-account auth for TypeScript

Node.js 22+ client for confidential applications. The private RSA key remains in the application’s KMS, HSM, TPM, secret manager, or protected filesystem; PromptFleet receives only the public key during credential registration.

Provide a `JwtSigner` backed by your key system, then request short-lived, audience- and resource-bound invoke tokens:

After PromptFleet activates the first registered public key, copy the service account's displayed OAuth client ID into `PF_SERVICE_ACCOUNT_CLIENT_ID`.

```ts
const client = new PromptFleetServiceAccountClient({
  clientId: process.env.PF_SERVICE_ACCOUNT_CLIENT_ID!,
  oauthTokenUrl: "https://identity.example/oauth/v2/token",
  oauthAudience: "https://identity.example",
  oauthScopes: ["urn:zitadel:iam:org:project:id:YOUR_PROMPTFLEET_PROJECT_ID:aud"],
  invokeTokenUrl: "https://api.promptfleet.ai/v1/invoke/token",
  signer: kmsSigner,
});

const token = await client.getInvokeToken({
  audience: "pf-workload-api",
  resource: "workload:wld-123",
  scopes: ["workload.invoke"],
});
```

`createNodePrivateKeySigner` is provided for development or protected-filesystem deployments. Prefer a non-exportable KMS/HSM signer in production.

`oauthAudience` is the ZITADEL custom-domain origin used in the signed assertion, not the token endpoint URL. `oauthScopes` must include the PromptFleet API project-audience scope shown by your platform administrator. The client uses the RFC 7523 JWT-bearer grant, adds the required `openid` scope, and refreshes the source token within five minutes even if ZITADEL reports a longer lifetime.

## Runnable Agent Edge example

[`examples/invoke-agent.ts`](examples/invoke-agent.ts) performs an authenticated
`GetAgentCard` request without printing any token or key material. Configure:

```bash
export PF_SERVICE_ACCOUNT_CLIENT_ID='...'
export PF_SERVICE_ACCOUNT_KEY_ID='...'
export PF_SERVICE_ACCOUNT_PRIVATE_KEY_FILE='/protected/path/private-key.pem'
export PF_OAUTH_PROJECT_SCOPE='urn:zitadel:iam:org:project:id:...:aud'
export PF_AGENT_EDGE_URL='https://my-agent.edge.promptfleet.ai/jsonrpc'
export PF_AGENT_RESOURCE='agent:A-...'
npm run example:agent
```
