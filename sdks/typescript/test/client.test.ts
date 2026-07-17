import assert from "node:assert/strict";
import test from "node:test";
import {
  PromptFleetServiceAccountClient,
  createInvokeFetch,
  type JwtSigner,
} from "../src/index.js";

test("exchanges a signed OAuth credential for a resource-bound invoke token and caches it", async () => {
  const requests: Array<{ url: string; form: URLSearchParams }> = [];
  const fetch = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = String(input);
    const form = new URLSearchParams(String(init?.body));
    requests.push({ url, form });
    if (url.endsWith("/oauth/v2/token")) {
      return Response.json({ access_token: "oauth-token", token_type: "Bearer", expires_in: 300 });
    }
    return Response.json({ access_token: "invoke-token", token_type: "Bearer", expires_in: 300 });
  };
  const signer: JwtSigner = { keyId: "provider-key", async sign() { return new Uint8Array([1, 2, 3]); } };
  const client = new PromptFleetServiceAccountClient({
    clientId: "machine-user-id",
    oauthTokenUrl: "https://identity.example/oauth/v2/token",
    oauthAudience: "https://identity.example",
    oauthScopes: ["urn:zitadel:iam:org:project:id:promptfleet:aud"],
    invokeTokenUrl: "https://api.example/v1/invoke/token",
    signer,
    fetch: fetch as typeof globalThis.fetch,
    clock: () => 1_000_000,
  });

  const first = await client.getInvokeToken({ audience: "pf-workload-api", resource: "workload:wld-1", scopes: ["workload.invoke"] });
  const second = await client.getInvokeToken({ audience: "pf-workload-api", resource: "workload:wld-1", scopes: ["workload.invoke"] });

  assert.equal(first.accessToken, "invoke-token");
  assert.equal(second.accessToken, "invoke-token");
  assert.equal(requests.length, 2);
  assert.equal(requests[0].form.get("grant_type"), "urn:ietf:params:oauth:grant-type:jwt-bearer");
  assert.ok(requests[0].form.get("assertion"));
  assert.match(requests[0].form.get("scope") ?? "", /(^| )openid( |$)/);
  assert.equal(requests[1].form.get("subject_token"), "oauth-token");
  assert.equal(requests[1].form.get("resource"), "workload:wld-1");
});

test("rejects an invoke request without resource binding", async () => {
  const client = new PromptFleetServiceAccountClient({
    clientId: "machine-user-id",
    oauthTokenUrl: "https://identity.example/oauth/v2/token",
    oauthAudience: "https://identity.example",
    oauthScopes: ["urn:zitadel:iam:org:project:id:promptfleet:aud"],
    invokeTokenUrl: "https://api.example/v1/invoke/token",
    signer: { keyId: "key", async sign() { return new Uint8Array(); } },
  });
  await assert.rejects(
    client.getInvokeToken({ audience: "pf-workload-api", resource: "", scopes: ["workload.invoke"] }),
    /resource/,
  );
});

test("composable invoke fetch refreshes auth without replacing client headers", async () => {
  const requests: RequestInit[] = [];
  const provider = {
    async getInvokeToken() {
      return { accessToken: "invoke-token", tokenType: "Bearer", expiresAt: Date.now() + 60_000 };
    },
  };
  const fetch = createInvokeFetch(
    provider,
    { audience: "agent-edge-gateway", resource: "agent:a-1", scopes: ["agent.invoke"] },
    async (_input, init) => {
      requests.push(init ?? {});
      return Response.json({ ok: true });
    },
  );

  await fetch("https://agent.edge.example/agui/v1/runs", {
    method: "POST",
    headers: { "x-client": "example" },
  });

  const headers = new Headers(requests[0].headers);
  assert.equal(headers.get("authorization"), "Bearer invoke-token");
  assert.equal(headers.get("x-client"), "example");
});
