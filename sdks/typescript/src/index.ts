import { createPrivateKey, KeyObject, randomUUID, sign as nodeSign } from "node:crypto";

const CLIENT_ASSERTION_TYPE = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";
export const DEFAULT_INVOKE_TRUST_TOKEN_URL = "https://issuer.promptfleet.ai/invoke-trust/token";

export interface JwtSigner {
  readonly keyId: string;
  sign(signingInput: Uint8Array): Promise<Uint8Array>;
}

export interface ServiceAccountClientOptions {
  clientId: string;
  tokenUrl?: string;
  signer: JwtSigner;
  fetch?: typeof globalThis.fetch;
  clock?: () => number;
}

export interface InvokeTokenRequest {
  resource: string;
  scopes: readonly string[];
}

export interface AccessToken {
  accessToken: string;
  tokenType: string;
  expiresAt: number;
  scope?: string;
}

export interface InvokeTokenProvider {
  getInvokeToken(request: InvokeTokenRequest): Promise<AccessToken>;
}

export type AuthenticatedFetch = (
  input: string | URL | globalThis.Request,
  init?: RequestInit,
) => Promise<Response>;

type TokenResponse = {
  access_token?: unknown;
  token_type?: unknown;
  expires_in?: unknown;
  scope?: unknown;
};

export class PromptFleetServiceAccountClient {
  readonly #options: ServiceAccountClientOptions;
  readonly #fetch: typeof globalThis.fetch;
  readonly #clock: () => number;
  readonly #invoke = new Map<string, AccessToken>();
  readonly #invokePending = new Map<string, Promise<AccessToken>>();

  constructor(options: ServiceAccountClientOptions) {
    this.#options = options;
    this.#fetch = options.fetch ?? globalThis.fetch;
    this.#clock = options.clock ?? Date.now;
    if (!options.clientId || !options.signer?.keyId) {
      throw new Error("clientId and a signer with keyId are required");
    }
  }

  async getInvokeToken(request: InvokeTokenRequest): Promise<AccessToken> {
    if (!request.resource || request.scopes.length === 0) {
      throw new Error("resource and at least one scope are required");
    }
    const key = JSON.stringify([request.resource, [...request.scopes].sort()]);
    const cached = this.#invoke.get(key);
    if (isFresh(cached, this.#clock())) return cached;
    const pending = this.#invokePending.get(key);
    if (pending) return pending;
    const next = this.#requestInvokeToken(request).finally(() => this.#invokePending.delete(key));
    this.#invokePending.set(key, next);
    const token = await next;
    this.#invoke.set(key, token);
    return token;
  }

  async fetchWithInvokeToken(
    input: string | URL | globalThis.Request,
    init: RequestInit | undefined,
    request: InvokeTokenRequest,
  ): Promise<Response> {
    return createInvokeFetch(this, request, this.#fetch)(input, init);
  }

  async #requestInvokeToken(request: InvokeTokenRequest): Promise<AccessToken> {
    const tokenUrl = this.#options.tokenUrl ?? DEFAULT_INVOKE_TRUST_TOKEN_URL;
    const nowSeconds = Math.floor(this.#clock() / 1000);
    const header = encodeJson({ alg: "RS256", typ: "JWT", kid: this.#options.signer.keyId });
    const claims = encodeJson({
      iss: this.#options.clientId,
      sub: this.#options.clientId,
      aud: tokenUrl,
      iat: nowSeconds,
      exp: nowSeconds + 60,
      jti: randomUUID(),
    });
    const signingInput = `${header}.${claims}`;
    const signature = await this.#options.signer.sign(new TextEncoder().encode(signingInput));
    const assertion = `${signingInput}.${base64Url(signature)}`;
    const form = new URLSearchParams({
      grant_type: "client_credentials",
      client_id: this.#options.clientId,
      client_assertion_type: CLIENT_ASSERTION_TYPE,
      client_assertion: assertion,
      resource: request.resource,
      scope: request.scopes.join(" "),
    });
    return this.#postToken(tokenUrl, form, "PromptFleet invoke token");
  }

  async #postToken(url: string, form: URLSearchParams, label: string): Promise<AccessToken> {
    const response = await this.#fetch(url, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: form,
    });
    const body = (await response.json()) as TokenResponse & { error?: unknown };
    if (!response.ok) throw new Error(`${label} request failed (${response.status}): ${String(body.error ?? "unknown error")}`);
    if (typeof body.access_token !== "string" || typeof body.expires_in !== "number") {
      throw new Error(`${label} response is malformed`);
    }
    return {
      accessToken: body.access_token,
      tokenType: typeof body.token_type === "string" ? body.token_type : "Bearer",
      expiresAt: this.#clock() + body.expires_in * 1000,
      scope: typeof body.scope === "string" ? body.scope : undefined,
    };
  }
}

export async function invokeAuthorizationHeaders(
  provider: InvokeTokenProvider,
  request: InvokeTokenRequest,
  headers?: HeadersInit,
): Promise<Headers> {
  const token = await provider.getInvokeToken(request);
  const authenticated = new Headers(headers);
  authenticated.set("authorization", `Bearer ${token.accessToken}`);
  return authenticated;
}

/**
 * Creates a lazy authenticated fetch suitable for standard clients such as
 * `@ag-ui/client` HttpAgent and `@a2a-js/sdk`. A fresh resource-bound invoke
 * token is resolved for every request; the provider performs safe caching and
 * single-flight refresh.
 */
export function createInvokeFetch(
  provider: InvokeTokenProvider,
  request: InvokeTokenRequest,
  fetchImpl: AuthenticatedFetch = globalThis.fetch,
): AuthenticatedFetch {
  return async (input, init) => {
    const headers = await invokeAuthorizationHeaders(provider, request, init?.headers);
    return fetchImpl(input, { ...init, headers });
  };
}

export function createNodePrivateKeySigner(keyId: string, privateKey: string | Buffer | KeyObject): JwtSigner {
  const key = privateKey instanceof KeyObject ? privateKey : createPrivateKey(privateKey);
  return {
    keyId,
    async sign(signingInput: Uint8Array): Promise<Uint8Array> {
      return nodeSign("RSA-SHA256", signingInput, key);
    },
  };
}

function isFresh(token: AccessToken | undefined, now: number): token is AccessToken {
  return token !== undefined && token.expiresAt - now > 30_000;
}

function encodeJson(value: unknown): string {
  return base64Url(Buffer.from(JSON.stringify(value), "utf8"));
}

function base64Url(value: Uint8Array): string {
  return Buffer.from(value).toString("base64url");
}
