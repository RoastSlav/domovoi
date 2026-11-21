import { calls, route } from "./apiMock";
import { nas } from "./fixtures";
import { ENTITIES, FACTS_PAGE, RELATIONS_PAGE } from "./knowledgeFixtures";

type Query = Record<string, unknown> | undefined;

/** The facts of the fixture page that match a request's `source` filter. */
export function factsAnswer(query: Query) {
  const source = query?.source;
  return {
    items: FACTS_PAGE.items.filter((fact) => source === undefined || fact.source === source),
    next_cursor: null,
  };
}

/** Answers the routes every knowledge page reads: hosts, session, facts, relations and entities. */
export function mockKnowledgeApi(): void {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/session", () => ({
    csrf_token: "token",
    user: { id: "me", username: "me" },
  }));
  route("GET /api/v1/knowledge/entities", () => ENTITIES);
  route("GET /api/v1/knowledge/facts", ({ params }) => factsAnswer(params?.query));
  route("GET /api/v1/knowledge/relations", () => RELATIONS_PAGE);
}

/** The calls made to `route`, oldest first. */
export function callsTo(key: string) {
  return calls.filter((call) => call.route === key);
}

/** The body of the last call to `route`; fails when there was none. */
export function lastBody(key: string): Record<string, unknown> {
  const last = callsTo(key).at(-1);
  if (!last) {
    throw new Error(`no call to ${key}`);
  }
  return last.init.body as Record<string, unknown>;
}
