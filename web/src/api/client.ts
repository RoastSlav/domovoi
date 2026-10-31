import createClient from "openapi-fetch";
import type { paths } from "./schema";

/**
 * The only way the UI talks to the hub (ADR 0006). Build-plan prompt P19 adds
 * the CSRF middleware and the 401 redirect.
 */
export const api = createClient<paths>({ baseUrl: "/", credentials: "same-origin" });
