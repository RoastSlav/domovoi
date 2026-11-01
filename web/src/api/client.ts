import createClient, { type Middleware } from "openapi-fetch";
import type { paths } from "./schema";
import { keys, queryClient } from "./queryClient";
import type { Session } from "./types";

/** The header the hub checks on every unsafe request (ADR 0006). */
export const CSRF_HEADER = "X-CSRF-Token";

const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

/** Where a 401 sends the user. */
export type AuthPage = "/login" | "/setup";

/** Requests whose 401 is an answer for the page that made them, not a lost session. */
const CREDENTIAL_PATHS = new Set(["/api/v1/auth/login", "/api/v1/auth/setup"]);

let onUnauthorized: (to: AuthPage) => void = (to) => window.location.assign(to);

/** Sets what happens on a 401; the app passes React Router's `navigate`. */
export function setUnauthorizedHandler(handler: (to: AuthPage) => void): void {
  onUnauthorized = handler;
}

/** Adds the session's CSRF token to every request that is not a read. */
export const csrfMiddleware: Middleware = {
  onRequest({ request }) {
    if (SAFE_METHODS.has(request.method)) {
      return undefined;
    }
    const token = queryClient.getQueryData<Session>(keys.session)?.csrf_token;
    if (token) {
      request.headers.set(CSRF_HEADER, token);
    }
    return request;
  },
};

async function errorCode(response: Response): Promise<string | undefined> {
  try {
    const body: unknown = await response.clone().json();
    if (typeof body === "object" && body !== null && "error" in body) {
      return String(body.error);
    }
  } catch {
    // Not JSON: treat it as a lost session.
  }
  return undefined;
}

/** Sends the user to login, or to setup while no account exists, when a session ends. */
export const authMiddleware: Middleware = {
  async onResponse({ request, response }) {
    if (response.status !== 401 || CREDENTIAL_PATHS.has(new URL(request.url).pathname)) {
      return undefined;
    }
    const to: AuthPage = (await errorCode(response)) === "setup_required" ? "/setup" : "/login";
    if (window.location.pathname !== to) {
      onUnauthorized(to);
    }
    return undefined;
  },
};

/**
 * The only way the UI talks to the hub (ADR 0006): typed from the OpenAPI document, with
 * the CSRF header and the 401 redirect.
 */
export const api = createClient<paths>({
  baseUrl: globalThis.location?.origin ?? "",
  credentials: "same-origin",
});
api.use(csrfMiddleware, authMiddleware);
