# PromptFleet confidential-client SDKs

- [`typescript`](./typescript): Node.js 22+ signing-callback client.
- [`python`](./python): Python 3.12+ signing-callback client.

Both clients authenticate a PromptFleet service account with an RSA private-key JWT at the configured OAuth server, then exchange the OAuth access token for a short-lived PromptFleet invoke token. After the first public key becomes active, use the OAuth client ID shown for the service account (the identity-provider machine-user ID) as the SDK client ID. A signer callback keeps KMS/HSM/TPM integration independent of any cloud vendor SDK.

These SDKs are for confidential server-side applications. Browser and other public clients must use the interactive authorization-code flow with PKCE and must not hold a service-account private key.

Runnable Agent Edge examples are available in
[`typescript/examples/invoke-agent.ts`](typescript/examples/invoke-agent.ts) and
[`python/examples/invoke_agent.py`](python/examples/invoke_agent.py).
