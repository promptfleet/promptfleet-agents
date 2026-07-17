import { createPrivateKey, KeyObject, randomUUID, sign as nodeSign } from "node:crypto";

const TOKEN_EXCHANGE_GRANT = "urn:ietf:params:oauth:grant-type:token-exchange";
const JWT_BEARER_GRANT = "urn:ietf:params:oauth:grant-type:jwt-bearer";
const ACCESS_TOKEN_TYPE = "urn:ietf:params:oauth:token-type:access_token";
const PF_INVOKE_TOKEN_TYPE = "urn:promptfleet:params:oauth:token-type:pf-invoke-jwt";

export interface JwtSigner {
  readonly keyId: string;
  sign(signingInput: Uint8Array): Promise<Uint8Array>;
}

export interface ServiceAccountClientOptions {
  clientId: string;
  oauthTokenUrl: string;
  oauthAudience: string;
  invokeTokenUrl: string;
  signer: JwtSigner;
  oauthScopes: readonly string[];
  fetch?: typeof globalThis.fetch;
  clock?: () => number;
}

export interface InvokeTokenRequest {
  audience: string;
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
  #oauth?: AccessToken;
  readonly #invoke = new Map<string, AccessToken>();
  #oauthPending?: Promise<AccessToken>;
  readonly #invokePending = new Map<string, Promise<AccessToken>>();

  constructor(options: ServiceAccountClientOptions) {
    this.#options = options;
    this.#fetch = options.fetch ?? globalThis.fetch;
    this.#clock = options.clock ?? Date.now;
    if (!options.clientId || !options.oauthTokenUrl || !options.oauthAudience || !options.invokeTokenUrl) {
      throw new Error("clientId, oauthTokenUrl, oauthAudience, and invokeTokenUrl are required");
    }
    if (!options.oauthScopes?.length) {
      throw new Error("oauthScopes must include the PromptFleet API audience scope");
    }
  }

  async getOAuthAccessToken(): Promise<AccessToken> {
    if (isFresh(this.#oauth, this.#clock())) return this.#oauth;
    if (this.#oauthPending) return this.#oauthPending;
    this.#oauthPending = this.#requestOAuthAccessToken().finally(() => {
      this.#oauthPending = undefined;
    });
    this.#oauth = await this.#oauthPending;
    return this.#oauth;
  }

  async getInvokeToken(request: InvokeTokenRequest): Promise<AccessToken> {
    if (!request.audience || !request.resource || request.scopes.length === 0) {
      throw new Error("audience, resource, and at least one scope are required");
    }
    const key = JSON.stringify([request.audience, request.resource, [...request.scopes].sort()]);
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

  async #requestOAuthAccessToken(): Promise<AccessToken> {
    const nowSeconds = Math.floor(this.#clock() / 1000);
    const header = encodeJson({ alg: "RS256", typ: "JWT", kid: this.#options.signer.keyId });
    const claims = encodeJson({
      iss: this.#options.clientId,
      sub: this.#options.clientId,
      aud: this.#options.oauthAudience,
      iat: nowSeconds,
      exp: nowSeconds + 60,
      jti: randomUUID(),
    });
    const signingInput = `${header}.${claims}`;
    const signature = await this.#options.signer.sign(new TextEncoder().encode(signingInput));
    const assertion = `${signingInput}.${base64Url(signature)}`;
    const form = new URLSearchParams({
      grant_type: JWT_BEARER_GRANT,
      assertion,
    });
    const scopes = new Set(["openid", ...this.#options.oauthScopes]);
    form.set("scope", [...scopes].join(" "));
    const token = await this.#postToken(this.#options.oauthTokenUrl, form, "OAuth token");
    return { ...token, expiresAt: Math.min(token.expiresAt, this.#clock() + 300_000) };
  }

  async #requestInvokeToken(request: InvokeTokenRequest): Promise<AccessToken> {
    const source = await this.getOAuthAccessToken();
    const form = new URLSearchParams({
      grant_type: TOKEN_EXCHANGE_GRANT,
      subject_token_type: ACCESS_TOKEN_TYPE,
      subject_token: source.accessToken,
      audience: request.audience,
      resource: request.resource,
      scope: request.scopes.join(" "),
      requested_token_type: PF_INVOKE_TOKEN_TYPE,
    });
    return this.#postToken(this.#options.invokeTokenUrl, form, "PromptFleet invoke token");
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
