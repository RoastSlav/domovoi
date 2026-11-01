import type { MiddlewareCallbackParams } from "openapi-fetch";
import { CSRF_HEADER, authMiddleware, csrfMiddleware, setUnauthorizedHandler } from "./client";
import { keys, queryClient } from "./queryClient";

const ORIGIN = window.location.origin;

function params(request: Request): MiddlewareCallbackParams {
  return { request, schemaPath: "", params: {}, id: "test", options: {} } as never;
}

async function sent(method: string): Promise<Request> {
  const request = new Request(`${ORIGIN}/api/v1/incidents/1/ack`, { method });
  const changed = await csrfMiddleware.onRequest?.(params(request));
  return changed instanceof Request ? changed : request;
}

function json401(code: string): Response {
  return new Response(JSON.stringify({ error: code, message: "no" }), {
    status: 401,
    headers: { "Content-Type": "application/json" },
  });
}

async function respond(path: string, response: Response): Promise<void> {
  const request = new Request(`${ORIGIN}${path}`);
  await authMiddleware.onResponse?.({ ...params(request), response });
}

afterEach(() => {
  queryClient.clear();
  window.history.replaceState(null, "", "/");
});

test("client_adds_csrf_header_to_mutations_only", async () => {
  queryClient.setQueryData(keys.session, {
    user: { id: "u1", username: "admin" },
    csrf_token: "token-123",
  });
  for (const method of ["POST", "PUT", "PATCH", "DELETE"]) {
    expect((await sent(method)).headers.get(CSRF_HEADER), method).toBe("token-123");
  }
  for (const method of ["GET", "HEAD"]) {
    expect((await sent(method)).headers.get(CSRF_HEADER), method).toBeNull();
  }
});

test("client_sends_no_csrf_header_without_a_session", async () => {
  expect((await sent("POST")).headers.get(CSRF_HEADER)).toBeNull();
});

test("unauthenticated_response_redirects_to_login", async () => {
  const go = vi.fn();
  setUnauthorizedHandler(go);

  await respond("/api/v1/incidents", json401("unauthorized"));
  expect(go).toHaveBeenLastCalledWith("/login");

  await respond("/api/v1/session", json401("setup_required"));
  expect(go).toHaveBeenLastCalledWith("/setup");

  go.mockClear();
  await respond("/api/v1/auth/login", json401("bad_credentials"));
  await respond("/api/v1/incidents", new Response("{}", { status: 200 }));
  expect(go, "a failed login stays on the page").not.toHaveBeenCalled();

  window.history.replaceState(null, "", "/login");
  await respond("/api/v1/session", json401("unauthorized"));
  expect(go, "already there").not.toHaveBeenCalled();
});
