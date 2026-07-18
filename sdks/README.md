# PromptFleet confidential-client SDKs

- [`typescript`](./typescript): Node.js 20+ signing-callback client.
- [`python`](./python): Python 3.11+ signing-callback client.

Both clients authenticate a PromptFleet-native service account directly at `https://issuer.promptfleet.ai/invoke-trust/token` with OAuth `client_credentials` and an RSA `private_key_jwt`, receiving a short-lived resource-bound PromptFleet invoke token. Use the displayed service-account client ID and registered key ID; no underlying identity-provider values are needed. A signer callback keeps KMS/HSM/TPM integration independent of any cloud vendor SDK.

These SDKs are for confidential server-side applications. Browser and other public clients must use the interactive authorization-code flow with PKCE and must not hold a service-account private key.

Runnable Agent Edge examples are available in
[`typescript/examples/invoke-agent.ts`](typescript/examples/invoke-agent.ts) and
[`python/examples/invoke_agent.py`](python/examples/invoke_agent.py).
