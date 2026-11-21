import { beforeEach, describe, expect, it, vi } from "vitest";
import { PROFILE_IDS } from "../../design/visibility";
import { resetApi } from "../../test/apiMock";
import { FACT_PURGED, FACT_RETRACTED, FACTS_PAGE } from "../../test/knowledgeFixtures";
import {
  ENTITIES_TAB,
  ENTITY_PAGE,
  EXPERIENCE_PAGE,
  FACTS_TAB,
  OPERATION_WITH_SUMMARY,
  PATHS_AVAILABLE,
  PATHS_PAGE,
  PROPOSALS_TAB,
  RELATIONS_TAB,
  USER_PATH,
  expectedFactIds,
  factScreen,
  serveScreens,
  type Screen,
} from "../../test/knowledgeScreens";
import { expectP0Everywhere } from "../../test/visibility";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  serveScreens();
});

const SLOW = 40_000;

async function everywhere(screen: Screen) {
  await expectP0Everywhere({
    render: (profile) => screen.render(profile),
    expectedIds: screen.ids,
  });
}

const byId = (id: string) => {
  const found = FACTS_PAGE.items.find((fact) => fact.id === id);
  if (!found) {
    throw new Error(`no fixture fact ${id}`);
  }
  return found;
};

describe("p0_visible_in_every_profile_for_the_knowledge_pages", () => {
  it(
    "shows the facts list and the waiting note in every profile",
    () => everywhere(FACTS_TAB),
    SLOW,
  );
  it("shows the proposals in every profile", () => everywhere(PROPOSALS_TAB), SLOW);
  it("shows the relations in every profile", () => everywhere(RELATIONS_TAB), SLOW);
  it("shows the entities in every profile", () => everywhere(ENTITIES_TAB), SLOW);
  it(
    "shows a retracted fact's state, status and note in every profile",
    async () => {
      await everywhere(factScreen(FACT_RETRACTED));
    },
    SLOW,
  );
  it(
    "shows a purged fact's note in every profile",
    () => everywhere(factScreen(FACT_PURGED)),
    SLOW,
  );
  it(
    "shows an inferred fact's buttons in every profile",
    async () => {
      await everywhere(factScreen(byId("fact-inferred-role")));
    },
    SLOW,
  );
  it(
    "shows a path fact's note and buttons in every profile",
    async () => {
      const path = USER_PATH;
      expect(expectedFactIds(path)).toEqual([
        "fact.state",
        "fact.status",
        "fact.actions",
        "fact.notes",
      ]);
      serveScreens();
      await everywhere(factScreen(path));
    },
    SLOW,
  );
  it(
    "shows an entity's actions, facts and relations in every profile",
    () => everywhere(ENTITY_PAGE),
    SLOW,
  );
  it(
    "shows the paths page while coverage is unavailable in every profile",
    () => everywhere(PATHS_PAGE),
    SLOW,
  );
  it(
    "shows the paths page when coverage can be checked in every profile",
    async () => {
      serveScreens(PATHS_AVAILABLE);
      await everywhere(PATHS_PAGE);
    },
    SLOW,
  );
  it(
    "shows the experience notice and results in every profile",
    () => everywhere(EXPERIENCE_PAGE),
    SLOW,
  );
  it(
    "shows a ready resolution summary in every profile",
    () => everywhere(OPERATION_WITH_SUMMARY),
    SLOW,
  );

  it("holds each P0 id in at least one case so the test cannot pass vacuously", () => {
    const covered = new Set(
      [
        FACTS_TAB,
        PROPOSALS_TAB,
        RELATIONS_TAB,
        ENTITIES_TAB,
        ENTITY_PAGE,
        PATHS_PAGE,
        EXPERIENCE_PAGE,
        OPERATION_WITH_SUMMARY,
        factScreen(FACT_RETRACTED),
        factScreen(byId("fact-inferred-role")),
        factScreen(USER_PATH),
      ].flatMap((screen) => screen.ids),
    );
    const table = [
      "knowledge.waiting-note",
      "knowledge.facts",
      "knowledge.proposals",
      "knowledge.relations",
      "knowledge.entities",
      "fact.state",
      "fact.status",
      "fact.actions",
      "fact.notes",
      "entity.state",
      "entity.actions",
      "paths.state",
      "paths.blocked",
      "paths.coverage",
      "paths.list",
      "experience.label",
      "experience.results",
      "experience.summary",
    ];
    expect(table.filter((id) => !covered.has(id))).toEqual([]);
  });
});

describe("p0_visible_in_every_profile_with_a_page_override", () => {
  it(
    "keeps the paths P0 blocks when the page is Essential and the default is Detailed",
    async () => {
      await expectP0Everywhere({
        render: (profile) =>
          PATHS_PAGE.render(profile, { paths: profile === "essential" ? "detailed" : "essential" }),
        expectedIds: PATHS_PAGE.ids,
      });
    },
    SLOW,
  );
  it(
    "keeps the experience P0 blocks when the page is Detailed and the default is Essential",
    async () => {
      await expectP0Everywhere({
        render: (profile) =>
          EXPERIENCE_PAGE.render(profile, {
            experience: profile === "detailed" ? "essential" : "detailed",
          }),
        expectedIds: EXPERIENCE_PAGE.ids,
      });
    },
    SLOW,
  );
  it("covers every profile as the default", () => {
    expect(PROFILE_IDS).toHaveLength(3);
  });
});
