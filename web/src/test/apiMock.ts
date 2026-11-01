/**
 * A stand-in for `src/api/client` in tests: `api.GET` and `api.POST` answer from routes
 * the test registers, so no test touches the network.
 *
 * Use with `vi.mock("<path>/api/client", () => import("<path>/test/apiMock"))`.
 */

type Init = {
  params?: { path?: Record<string, string>; query?: Record<string, unknown> };
  body?: unknown;
  bodySerializer?: (body: unknown) => unknown;
};
type Handler = (init: Init) => unknown;

interface Answer {
  data?: unknown;
  error?: unknown;
  response: Response;
}

const routes = new Map<string, Handler>();

/** Calls made, as `METHOD path` with their init. */
export const calls: { route: string; init: Init }[] = [];

/** Answers `METHOD path` (the OpenAPI path, for example `/api/v1/incidents/{id}`). */
export function route(key: string, handler: Handler): void {
  routes.set(key, handler);
}

/** A failed call's answer; return it from a handler. */
export function failure(
  status: number,
  error: string,
  message: string,
  headers?: HeadersInit,
  issues?: { path: string; message: string; severity: string }[],
) {
  return new Failure(status, error, message, headers, issues);
}

class Failure {
  constructor(
    readonly status: number,
    readonly error: string,
    readonly message: string,
    readonly headers?: HeadersInit,
    readonly issues?: { path: string; message: string; severity: string }[],
  ) {}
}

/** Forgets routes and calls. */
export function resetApi(): void {
  routes.clear();
  calls.length = 0;
}

async function answer(method: string, path: string, init: Init = {}): Promise<Answer> {
  const key = `${method} ${path}`;
  calls.push({ route: key, init });
  const handler = routes.get(key);
  if (!handler) {
    throw new Error(`no test route for ${key}`);
  }
  const result = handler(init);
  if (result instanceof Failure) {
    return {
      error: { error: result.error, message: result.message, issues: result.issues ?? [] },
      response: new Response(null, { status: result.status, headers: result.headers ?? {} }),
    };
  }
  return { data: result, response: new Response(null, { status: 200 }) };
}

/** The mocked client. */
export const api = {
  GET: (path: string, init?: Init) => answer("GET", path, init),
  POST: (path: string, init?: Init) => answer("POST", path, init),
  PUT: (path: string, init?: Init) => answer("PUT", path, init),
  PATCH: (path: string, init?: Init) => answer("PATCH", path, init),
  DELETE: (path: string, init?: Init) => answer("DELETE", path, init),
};

/** The CSRF header name, as the real module exports it. */
export const CSRF_HEADER = "X-CSRF-Token";

/** Does nothing in tests. */
export function setUnauthorizedHandler(): void {}
