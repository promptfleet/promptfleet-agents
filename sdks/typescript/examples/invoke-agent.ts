import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";

import {
  PromptFleetServiceAccountClient,
  createNodePrivateKeySigner,
} from "../src/index.js";

function required(name: string): string {
  const value = process.env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

const privateKey = await readFile(required("PF_SERVICE_ACCOUNT_PRIVATE_KEY_FILE"));
const client = new PromptFleetServiceAccountClient({
  clientId: required("PF_SERVICE_ACCOUNT_CLIENT_ID"),
  tokenUrl: process.env.PF_OAUTH_TOKEN_URL,
  signer: createNodePrivateKeySigner(required("PF_SERVICE_ACCOUNT_KEY_ID"), privateKey),
});

const response = await client.fetchWithInvokeToken(
  required("PF_AGENT_EDGE_URL"),
  {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `service-account-example-${randomUUID()}`,
      method: "GetAgentCard",
      params: null,
    }),
  },
  {
    resource: required("PF_AGENT_RESOURCE"),
    scopes: ["agent.invoke"],
  },
);

const body = await response.text();
if (!response.ok) throw new Error(`Agent Edge request failed (${response.status}): ${body}`);

const payload = JSON.parse(body) as { result?: { name?: unknown }; error?: unknown };
if (payload.error || typeof payload.result?.name !== "string") {
  throw new Error(`Agent Edge returned an invalid AgentCard response: ${body}`);
}

console.log(`Authenticated AgentCard: ${payload.result.name}`);
