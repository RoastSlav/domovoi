import { screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import type { KnowledgeFact, ManagementPaths } from "../api/types";
import { EntityDetailPage } from "../features/knowledge/EntityDetailPage";
import { FactDetailPage } from "../features/knowledge/FactDetailPage";
import { KnowledgePage } from "../features/knowledge/KnowledgePage";
import { PathsPage } from "../features/knowledge/PathsPage";
import { ExperiencePage } from "../features/experience/ExperiencePage";
import type { ProfileId } from "../design/visibility";
import { failure, route } from "./apiMock";
import searchHost from "./fixtures/experience/search-host.json";
import summaryReady from "./fixtures/experience/summary-ready.json";
import { nas } from "./fixtures";
import {
  FACT_PURGED,
  FACT_RETRACTED,
  FACTS_PAGE,
  PATHS_AVAILABLE,
  PATHS_UNAVAILABLE,
} from "./knowledgeFixtures";
import { mockKnowledgeApi } from "./knowledgeRoutes";
import { operationDetail } from "./operationFixtures";
import { showOperation } from "./operationPage";
import { renderWithProfile } from "./visibility";

/** Every fact the fixtures hold, by id, for the detail route. */
const FACTS: readonly KnowledgeFact[] = [
  ...FACTS_PAGE.items,
  FACT_RETRACTED,
  FACT_PURGED,
  ...PATHS_UNAVAILABLE.paths.map((path) => path.fact),
];

/** A management path the owner entered and has not confirmed. */
export const USER_PATH: KnowledgeFact = knowledgePathFact("path-unconfirmed");

function knowledgePathFact(id: string): KnowledgeFact {
  const found = PATHS_UNAVAILABLE.paths.find((path) => path.fact.id === id);
  if (!found) {
    throw new Error(`no fixture path ${id}`);
  }
  return found.fact;
}

/** Answers every route the knowledge, paths and experience screens read. */
export function serveScreens(paths: ManagementPaths = PATHS_UNAVAILABLE): void {
  mockKnowledgeApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/knowledge/facts/{id}", ({ params }) => {
    const found = FACTS.find((fact) => fact.id === params?.path?.id);
    return found ?? failure(404, "not_found", "no such fact");
  });
  route("GET /api/v1/knowledge/facts", ({ params }) => {
    const query = params?.query;
    if (query?.status === "superseded") {
      return { items: [], next_cursor: null };
    }
    const source = query?.source;
    return {
      items: FACTS_PAGE.items.filter((fact) => source === undefined || fact.source === source),
      next_cursor: null,
    };
  });
  route("GET /api/v1/knowledge/hosts/{name}/management-paths", () => paths);
  route("GET /api/v1/experience/search", () => searchHost);
}

/** The path template of each screen's route. */
const ROUTES = {
  facts: "/knowledge",
  fact: "/knowledge/facts/:id",
  entity: "/knowledge/entities/:id",
  paths: "/knowledge/paths",
  experience: "/knowledge/experience",
} as const;

/** One screen of the pages under test: how to draw it and which P0 blocks it must show. */
export interface Screen {
  ids: string[];
  render: (
    profile: ProfileId,
    pages?: Record<string, ProfileId>,
  ) => Promise<{ unmount: () => void }>;
}

/** Draws `element` at `url` and waits until every id in `ids` is on screen. */
function screenOf(
  url: string,
  path: keyof typeof ROUTES,
  element: ReactNode,
  ids: string[],
): Screen {
  return {
    ids,
    async render(profile, pages) {
      const view = renderWithProfile(profile, url, ROUTES[path], element, pages);
      await waitFor(() => {
        for (const id of ids) {
          expect(document.querySelector(`[data-visibility-id="${id}"]`), id).not.toBeNull();
        }
      });
      return view;
    },
  };
}

const LIVE = new Set(["current", "stale", "disputed"]);

/**
 * The P0 blocks a fact page must show, from the product rules rather than from the page's own
 * helpers: state and status always; the owner's buttons while the fact is live; a note for a
 * retracted, replaced or purged fact or for a path the owner entered.
 */
export function expectedFactIds(fact: KnowledgeFact): string[] {
  const live = LIVE.has(fact.status);
  const ids = ["fact.state", "fact.status"];
  if (live) {
    ids.push("fact.actions");
  }
  const note =
    fact.status === "retracted" ||
    fact.status === "superseded" ||
    fact.content_purged ||
    (live && fact.predicate === "management_path" && fact.source === "user");
  if (note) {
    ids.push("fact.notes");
  }
  return ids;
}

/** The detail page of a fixture fact. */
export function factScreen(fact: KnowledgeFact): Screen {
  return screenOf(`/knowledge/facts/${fact.id}`, "fact", <FactDetailPage />, expectedFactIds(fact));
}

const inferred = FACTS_PAGE.items.filter((fact) => fact.source === "inferred");

/** The facts tab; it shows the waiting note exactly when some fact is inferred. */
export const FACTS_TAB = screenOf(
  "/knowledge?host=nas",
  "facts",
  <KnowledgePage />,
  inferred.length > 0 ? ["knowledge.facts", "knowledge.waiting-note"] : ["knowledge.facts"],
);
/** The proposals tab. */
export const PROPOSALS_TAB = screenOf(
  "/knowledge?tab=proposals&host=nas",
  "facts",
  <KnowledgePage />,
  ["knowledge.proposals"],
);
/** The relations tab. */
export const RELATIONS_TAB = screenOf("/knowledge?tab=relations", "facts", <KnowledgePage />, [
  "knowledge.relations",
]);
/** The entities tab. */
export const ENTITIES_TAB = screenOf("/knowledge?tab=entities", "facts", <KnowledgePage />, [
  "knowledge.entities",
]);
/** An entity's page, with its facts and relations. */
export const ENTITY_PAGE = screenOf("/knowledge/entities/ent-vpn", "entity", <EntityDetailPage />, [
  "entity.state",
  "entity.actions",
  "knowledge.facts",
  "knowledge.relations",
]);
/** The management paths page of `nas`. */
export const PATHS_PAGE = screenOf("/knowledge/paths?host=nas", "paths", <PathsPage />, [
  "paths.state",
  "paths.blocked",
  "paths.coverage",
  "paths.list",
]);
/** The experience search on `nas`. */
export const EXPERIENCE_PAGE = screenOf(
  "/knowledge/experience?host=nas",
  "experience",
  <ExperiencePage />,
  ["experience.label", "experience.results"],
);

/** A finished operation's page with a ready resolution summary. */
export const OPERATION_WITH_SUMMARY: Screen = {
  ids: ["experience.summary"],
  async render(profile, pages) {
    const view = showOperation(operationDetail("verified-success"), {
      profile,
      pages: pages ?? {},
      summary: () => summaryReady,
    });
    await waitFor(() =>
      expect(document.querySelector('[data-visibility-id="experience.summary"]')).not.toBeNull(),
    );
    await screen.findByText("Resolution summary");
    return view;
  },
};

/** The paths fixtures, so a test can serve the second one. */
export { PATHS_AVAILABLE, PATHS_UNAVAILABLE };
