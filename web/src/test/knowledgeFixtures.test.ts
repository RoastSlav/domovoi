import type { KnowledgeFact } from "../api/types";
import { HISTORICAL_LABEL } from "../design/knowledgeText";
import {
  ENTITIES,
  FACT_PURGED,
  FACT_RETRACTED,
  FACTS_PAGE,
  PATHS_AVAILABLE,
  PATHS_UNAVAILABLE,
  RELATIONS_PAGE,
  knowledgeEntity,
  knowledgeFact,
  knowledgeRelation,
  managementPath,
} from "./knowledgeFixtures";
import search from "./fixtures/experience/search-host.json";
import similar from "./fixtures/experience/similar.json";
import summary from "./fixtures/experience/summary-ready.json";

const KINDS = /^(host|container|unit|entity):.+/;

function unique(ids: string[]) {
  expect(new Set(ids).size).toBe(ids.length);
}

describe("knowledge fixtures", () => {
  it("each_knowledge_fixture_has_the_generated_type", () => {
    const facts: KnowledgeFact[] = [
      ...FACTS_PAGE.items,
      FACT_RETRACTED,
      FACT_PURGED,
      ...PATHS_UNAVAILABLE.paths.map((p) => p.fact),
      ...PATHS_AVAILABLE.paths.map((p) => p.fact),
      knowledgeFact(),
      managementPath().fact,
    ];
    unique(FACTS_PAGE.items.map((f) => f.id));
    for (const fact of facts) {
      expect(fact.subject).toMatch(KINDS);
      expect(fact.id).not.toBe("");
      expect(typeof fact.version).toBe("number");
    }
    for (const relation of [...RELATIONS_PAGE.items, knowledgeRelation()]) {
      expect(relation.from).toMatch(KINDS);
      expect(relation.to).toMatch(KINDS);
    }
    unique(RELATIONS_PAGE.items.map((r) => r.id));
    unique(ENTITIES.map((e) => e.id));
    expect(knowledgeEntity().kind).toBe("vpn");
  });

  it("the_fixtures_cover_each_state_the_pages_need", () => {
    expect(PATHS_UNAVAILABLE.coverage_available).toBe(false);
    expect(PATHS_AVAILABLE.coverage_available).toBe(true);
    expect(PATHS_UNAVAILABLE.paths.map((p) => p.state)).toEqual([
      "candidate",
      "unconfirmed",
      "stale",
    ]);
    expect(PATHS_AVAILABLE.paths.map((p) => p.state)).toEqual(["unconfirmed", "confirmed"]);
    for (const path of [...PATHS_UNAVAILABLE.paths, ...PATHS_AVAILABLE.paths]) {
      expect(path.fact.predicate).toBe("management_path");
      expect(path.state === "confirmed").toBe(path.confirmation !== null);
    }
    expect(FACT_PURGED.content_purged).toBe(true);
    expect(FACT_PURGED.value ?? null).toBeNull();
    expect(FACT_RETRACTED.status).toBe("retracted");
    const statuses = new Set(FACTS_PAGE.items.map((f) => f.status));
    expect(statuses).toEqual(new Set(["current", "stale", "disputed"]));
    expect(FACTS_PAGE.items.filter((f) => f.source === "inferred").length).toBeGreaterThan(0);
    expect(FACTS_PAGE.items.filter((f) => f.status === "disputed")).toHaveLength(2);
  });

  it("every_past_operation_fixture_carries_the_historical_label", () => {
    for (const fixture of [search, similar, summary]) {
      expect(fixture.label).toBe(HISTORICAL_LABEL);
    }
  });
});
