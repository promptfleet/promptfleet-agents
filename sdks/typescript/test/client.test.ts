import assert from "node:assert/strict";
import test from "node:test";
import {
  PromptFleetServiceAccountClient,
  createInvokeFetch,
  type JwtSigner,
} from "../src/index.js";

test("authenticates with private_key_jwt and caches the resource-bound invoke token", async () => {
  const requests: Array<{ url: string; form: URLSearchParams }> = [];
  const fetch = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = String(input);
    const form = new URLSearchParams(String(init?.body));
    requests.push({ url, form });
    return Response.json({ access_token: "invoke-token", token_type: "Bearer", expires_in: 300 });
  };
  const signer: JwtSigner = { keyId: "provider-key", async sign() { return new Uint8Array([1, 2, 3]); } };
  const client = new PromptFleetServiceAccountClient({
    clientId: "svc:test",
    tokenUrl: "https://issuer.promptfleet.ai/invoke-trust/token",
    signer,
    fetch: fetch as typeof globalThis.fetch,
    clock: () => 1_000_000,
  });

  const first = await client.getInvokeToken({ resource: "https://api.promptfleet.ai/workloads", scopes: ["workload.invoke"] });
  const second = await client.getInvokeToken({ resource: "https://api.promptfleet.ai/workloads", scopes: ["workload.invoke"] });

  assert.equal(first.accessToken, "invoke-token");
  assert.equal(second.accessToken, "invoke-token");
  assert.equal(requests.length, 1);
  assert.equal(requests[0].form.get("grant_type"), "client_credentials");
  assert.equal(requests[0].form.get("client_id"), "svc:test");
  assert.ok(requests[0].form.get("client_assertion"));
  assert.equal(requests[0].form.get("resource"), "https://api.promptfleet.ai/workloads");
});

test("rejects an invoke request without resource binding", async () => {
  const client = new PromptFleetServiceAccountClient({
    clientId: "svc:test",
    signer: { keyId: "key", async sign() { return new Uint8Array(); } },
  });
  await assert.rejects(
    client.getInvokeToken({ resource: "", scopes: ["workload.invoke"] }),
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
    { resource: "https://demo.edge.promptfleet.ai", scopes: ["agent.invoke"] },
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
