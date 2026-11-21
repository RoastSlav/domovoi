import type { QueryClient } from "@tanstack/react-query";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  PolicyOverview,
  PolicyPreview,
  PolicySimulation,
  ShadowDetail,
} from "../../api/types";
import { PROFILE_IDS, type ProfileId } from "../../design/visibility";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { changeNote, hub } from "../../test/draftPage";
import {
  DECISION_EXCEPTION,
  DECISION_LIMITS,
  DECISION_LOCKED,
  DECISION_UNAVAILABLE,
  DECISION_UNEVALUABLE,
  DRAFT_4,
  OVERVIEW_ACTIVE,
  OVERVIEW_BERLIN,
  OVERVIEW_FRESH,
  PREVIEW_BLOCKING,
  PREVIEW_INVALID,
  SHADOW_PAGE_ONE,
  SIMULATE_BOTH,
  VERSION_3,
} from "../../test/policyFixtures";
import { expectP0Everywhere, p0Elements, renderWithProfile } from "../../test/visibility";
import { DecisionPage } from "./DecisionPage";
import { DraftPage } from "./DraftPage";
import { PolicyPage } from "./PolicyPage";
import { PublishPage } from "./PublishPage";
import { SimulatorPage } from "./SimulatorPage";
import { VersionPage } from "./VersionPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  localStorage.clear();
});

const OVERVIEW = "GET /api/v1/policy";
const VERSION = "GET /api/v1/policy/versions/{number}";
const VERSIONS = "GET /api/v1/policy/versions";
const SHADOW = "GET /api/v1/policy/shadow";
const DECISION = "GET /api/v1/policy/shadow/{execution_id}";
const DRAFT = "GET /api/v1/policy/draft";
const PREVIEW = "POST /api/v1/policy/preview";
const PUBLISH = "POST /api/v1/policy/draft/publish";
const SIMULATE = "POST /api/v1/policy/simulate";
const HOSTS = "GET /api/v1/hosts";

const SLOW = 240_000;

// ---------------------------------------------------------------------------------------------
// What each screen must show, worked out from the fixtures and the product rules, not from the
// code under test.
// ---------------------------------------------------------------------------------------------

type Tab = "rules" | "decisions";

function overviewIds(overview: PolicyOverview, tab: Tab): string[] {
  const stale =
    overview.draft && (overview.draft.based_on ?? null) !== (overview.active?.number ?? null);
  return [
    "policy.state",
    "policy.mode",
    "policy.shadow",
    "policy.active",
    ...(stale ? ["policy.draft-stale"] : []),
    tab === "rules" ? "policy.rules" : "policy.decision-rows",
  ];
}

function publishIds(preview: PolicyPreview): string[] {
  return [
    "policy.mode",
    "publish.findings",
    "publish.controls",
    ...(preview.validation.length > 0 ? ["publish.validation"] : []),
    ...(preview.locked_changes.length > 0 ? ["publish.locks"] : []),
    ...((preview.simulation?.newly_allowed ?? 0) > 0 ? ["publish.widening"] : []),
  ];
}

function decisionIds(detail: ShadowDetail): string[] {
  if (detail.status !== "evaluated") return ["decision.state"];
  const isDocument = (side: unknown) =>
    typeof side === "object" && side !== null && "outcome" in side;
  const ids = ["decision.outcome", "decision.gaps"];
  if (isDocument(detail.enforced) || isDocument(detail.shadow)) {
    ids.push("decision.locked", "decision.exception", "decision.why", "decision.limits");
  }
  return ids;
}

function draftIds(overview: PolicyOverview, preview: PolicyPreview, dirty: boolean): string[] {
  const stale = (overview.draft?.based_on ?? null) !== (overview.active?.number ?? null);
  return [
    "draft.rules",
    ...(dirty ? ["draft.banner"] : []),
    ...(preview.validation.length > 0 ? ["draft.validation"] : []),
    ...(stale ? ["policy.draft-stale"] : []),
  ];
}

function simulatorIds(simulation: PolicySimulation): string[] {
  return [
    "simulator.basis",
    ...(simulation.summary.newly_allowed > 0 ? ["simulator.widening"] : []),
  ];
}

// ---------------------------------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------------------------------

type Pages = Record<string, ProfileId>;
type View = { client: QueryClient; unmount: () => void };

const present = (id: string) => document.querySelector(`[data-visibility-id="${id}"]`) !== null;

/** Waits until every id is on screen and every request has been answered. */
async function ready(view: View, ids: readonly string[]) {
  await waitFor(() => {
    for (const id of ids) expect(present(id), `${id} is on screen`).toBe(true);
  });
  await waitFor(() => expect(view.client.isFetching()).toBe(0));
}

function routeOverview(overview: PolicyOverview) {
  route(OVERVIEW, () => overview);
  route(VERSION, () => VERSION_3);
  route(VERSIONS, () => ({ items: [VERSION_3.summary], next_cursor: null }));
  route(SHADOW, () => ({ ...SHADOW_PAGE_ONE, next_cursor: null }));
  route(HOSTS, () => []);
}

async function showOverview(overview: PolicyOverview, tab: Tab, profile: ProfileId, pages?: Pages) {
  routeOverview(overview);
  const url = tab === "rules" ? "/policy" : `/policy?tab=${tab}`;
  const view = renderWithProfile(profile, url, "/policy", <PolicyPage />, pages);
  await ready(view, overviewIds(overview, tab));
  return view;
}

async function showVersion(profile: ProfileId, pages?: Pages) {
  routeOverview({ ...OVERVIEW_ACTIVE, draft: null });
  const view = renderWithProfile(
    profile,
    "/policy/versions/3",
    "/policy/versions/:number",
    <VersionPage />,
    pages,
  );
  await ready(view, ["version.status", "version.acknowledgements"]);
  return view;
}

/** The editor after an edit, a failed check and a rejected save, so the banner and the validation show. */
async function showEditor(profile: ProfileId, pages?: Pages) {
  hub.draft = DRAFT_4;
  hub.preview = PREVIEW_INVALID;
  route(OVERVIEW, () => OVERVIEW_ACTIVE);
  route(DRAFT, () => hub.draft);
  route(HOSTS, () => []);
  route(PREVIEW, () => hub.preview);
  const view = renderWithProfile(profile, "/policy/draft", "/policy/draft", <DraftPage />, pages);
  const user = userEvent.setup();
  await changeNote(user, "ask-containers", "A note written in the test");
  await user.click(await screen.findByRole("button", { name: "Check rules" }));
  await screen.findByText(/Checking changes nothing/);
  route(PREVIEW, () =>
    failure(400, "invalid", "invalid rules", undefined, [
      { path: "rules.ask-containers", message: "Unknown category.", severity: "error" },
    ]),
  );
  await user.click(screen.getByRole("button", { name: "Save draft" }));
  await ready(view, draftIds(OVERVIEW_ACTIVE, PREVIEW_INVALID, true));
  return view;
}

async function showEmptyEditor(profile: ProfileId) {
  route(OVERVIEW, () => OVERVIEW_FRESH);
  route(HOSTS, () => []);
  const view = renderWithProfile(profile, "/policy/draft", "/policy/draft", <DraftPage />);
  await ready(view, ["policy.state"]);
  return view;
}

async function showPublish(preview: PolicyPreview, profile: ProfileId, pages?: Pages) {
  route(OVERVIEW, () => OVERVIEW_ACTIVE);
  route(DRAFT, () => DRAFT_4);
  route(PREVIEW, () => preview);
  route(HOSTS, () => []);
  const view = renderWithProfile(
    profile,
    "/policy/draft/publish",
    "/policy/draft/publish",
    <PublishPage />,
    pages,
  );
  await ready(view, publishIds(preview));
  return view;
}

async function showDecision(
  detail: ShadowDetail,
  profile: ProfileId,
  pages?: Pages,
  overview: PolicyOverview = OVERVIEW_ACTIVE,
) {
  route(DECISION, () => detail);
  route(OVERVIEW, () => overview);
  route(HOSTS, () => []);
  const view = renderWithProfile(
    profile,
    `/policy/decisions/${detail.execution_id}`,
    "/policy/decisions/:execution",
    <DecisionPage />,
    pages,
  );
  await ready(view, decisionIds(detail));
  return view;
}

function routeSimulator(overview: PolicyOverview) {
  route(OVERVIEW, () => overview);
  route(VERSIONS, () => ({
    items: overview.active ? [overview.active] : [],
    next_cursor: null,
  }));
  route(HOSTS, () => []);
  route(SIMULATE, () => SIMULATE_BOTH);
}

async function showSimulator(profile: ProfileId) {
  routeSimulator(OVERVIEW_ACTIVE);
  const view = renderWithProfile(
    profile,
    "/policy/simulator",
    "/policy/simulator",
    <SimulatorPage />,
  );
  await userEvent.click(await screen.findByRole("button", { name: "Run simulation" }));
  await ready(view, simulatorIds(SIMULATE_BOTH));
  return view;
}

async function showEmptySimulator(profile: ProfileId) {
  routeSimulator(OVERVIEW_FRESH);
  const view = renderWithProfile(
    profile,
    "/policy/simulator",
    "/policy/simulator",
    <SimulatorPage />,
  );
  await ready(view, ["simulator.state"]);
  return view;
}

interface Screen {
  expected: string[];
  name: string;
  show: (profile: ProfileId) => Promise<View>;
}

const SCREENS: Screen[] = [
  {
    name: "overview, rules tab, with a stale draft",
    expected: overviewIds(OVERVIEW_BERLIN, "rules"),
    show: (profile) => showOverview(OVERVIEW_BERLIN, "rules", profile),
  },
  {
    name: "overview, decisions tab",
    expected: overviewIds(OVERVIEW_ACTIVE, "decisions"),
    show: (profile) => showOverview(OVERVIEW_ACTIVE, "decisions", profile),
  },
  {
    name: "version",
    expected: ["version.status", "version.acknowledgements"],
    show: (profile) => showVersion(profile),
  },
  {
    name: "editor after an edit, a failed check and a rejected save",
    expected: draftIds(OVERVIEW_ACTIVE, PREVIEW_INVALID, true),
    show: (profile) => showEditor(profile),
  },
  { name: "editor without a draft", expected: ["policy.state"], show: showEmptyEditor },
  {
    name: "publication with blocking findings",
    expected: publishIds(PREVIEW_BLOCKING),
    show: (profile) => showPublish(PREVIEW_BLOCKING, profile),
  },
  {
    name: "publication with invalid rules",
    expected: publishIds(PREVIEW_INVALID),
    show: (profile) => showPublish(PREVIEW_INVALID, profile),
  },
  ...[
    ["locked", DECISION_LOCKED],
    ["exception", DECISION_EXCEPTION],
    ["limits", DECISION_LIMITS],
    ["unavailable", DECISION_UNAVAILABLE],
    ["unevaluable", DECISION_UNEVALUABLE],
  ].map(([name, detail]): Screen => {
    const fixture = detail as ShadowDetail;
    return {
      name: `decision, ${name as string}`,
      expected: decisionIds(fixture),
      show: (profile) => showDecision(fixture, profile),
    };
  }),
  {
    name: "simulator after a run",
    expected: simulatorIds(SIMULATE_BOTH),
    show: showSimulator,
  },
  {
    name: "simulator with nothing to simulate",
    expected: ["simulator.state"],
    show: showEmptySimulator,
  },
];

/** What the table of P0 elements says must appear somewhere. */
const REQUIRED = [
  "policy.shadow",
  "draft.banner",
  "draft.validation",
  "publish.findings",
  "publish.locks",
  "publish.widening",
  "decision.locked",
  "decision.exception",
  "decision.limits",
  "decision.gaps",
  "simulator.basis",
  "simulator.widening",
];

const idsOnScreen = () => new Set(p0Elements().map((el) => el.dataset["visibilityId"] ?? ""));

describe("P0 is visible in every profile on every policy screen", () => {
  it(
    "p0_visible_in_every_profile_on_every_policy_screen",
    async () => {
      const produced = new Set<string>();
      for (const screen of SCREENS) {
        expect(screen.expected.length, `${screen.name} expects something`).toBeGreaterThan(0);
        try {
          await expectP0Everywhere({
            expectedIds: screen.expected,
            render: async (profile) => {
              resetApi();
              const view = await screen.show(profile);
              for (const id of idsOnScreen()) produced.add(id);
              return view;
            },
          });
        } catch (error) {
          throw new Error(
            `${screen.name}: ${error instanceof Error ? error.message : String(error)}`,
            {
              cause: error,
            },
          );
        }
      }
      for (const id of REQUIRED) {
        expect(produced.has(id), `${id} is shown by some policy screen`).toBe(true);
      }
    },
    SLOW,
  );

  it(
    "p0_visible_in_every_profile_with_a_page_override",
    async () => {
      // Detailed default with an Essential page, Standard with Essential, Essential with Detailed.
      const other = (profile: ProfileId): ProfileId =>
        profile === "essential" ? "detailed" : "essential";
      const overridden = (profile: ProfileId): Pages => ({
        "policy-draft": other(profile),
        "policy-decision": other(profile),
      });
      const screens: {
        expected: string[];
        name: string;
        show: (p: ProfileId, pages: Pages) => Promise<View>;
      }[] = [
        {
          name: "editor",
          expected: draftIds(OVERVIEW_ACTIVE, PREVIEW_INVALID, true),
          show: (profile, pages) => showEditor(profile, pages),
        },
        {
          name: "publication",
          expected: publishIds(PREVIEW_BLOCKING),
          show: (profile, pages) => showPublish(PREVIEW_BLOCKING, profile, pages),
        },
        {
          name: "decision",
          expected: decisionIds(DECISION_LOCKED),
          show: (profile, pages) => showDecision(DECISION_LOCKED, profile, pages),
        },
      ];
      for (const screen of screens) {
        await expectP0Everywhere({
          expectedIds: screen.expected,
          render: async (profile) => {
            resetApi();
            return screen.show(profile, overridden(profile));
          },
        });
      }
    },
    SLOW,
  );

  it("a_page_override_really_changes_what_is_folded_on_the_decision_page", async () => {
    const facts = (id: string) => document.querySelector(`[data-visibility-id="${id}"]`);
    const view = await showDecision(DECISION_EXCEPTION, "detailed", {
      "policy-decision": "essential",
    });
    expect(screen.getByRole("button", { name: "+4 details" })).toBeInTheDocument();
    expect(facts("decision.facts")?.getAttribute("data-presentation")).toBe("collapsed");
    view.unmount();
    resetApi();
    await showDecision(DECISION_EXCEPTION, "essential", { "policy-decision": "detailed" });
    expect(screen.queryByRole("button", { name: "+4 details" })).toBeNull();
    expect(screen.getByText("Evaluated at")).toBeInTheDocument();
    expect(facts("decision.facts")?.getAttribute("data-presentation")).toBe("shown");
  });
});

// ---------------------------------------------------------------------------------------------
// What a profile folds, and what it never does
// ---------------------------------------------------------------------------------------------

function factNames(detail: ShadowDetail): string[] {
  const side = detail.enforced as { facts_used?: { fact: string }[] } | null;
  return (side?.facts_used ?? []).map((f) => f.fact);
}

describe("profiles on the decision page", () => {
  it("standard_collapses_decision_facts_and_inputs_and_essential_hides_metadata_behind_plus_details", async () => {
    const names = factNames(DECISION_EXCEPTION);
    expect(names.length).toBeGreaterThan(0);
    expect(DECISION_EXCEPTION.inputs).toBeTruthy();
    const inputs = () => document.querySelector('[data-visibility-id="decision.inputs"]');
    const allFacts = () => [...document.querySelectorAll('[data-visibility-id="decision.facts"]')];

    let view = await showDecision(DECISION_EXCEPTION, "detailed");
    expect(screen.getAllByText(names[0] as string).length).toBeGreaterThan(0);
    expect(inputs()?.getAttribute("data-presentation")).toBe("shown");
    expect(screen.getByText("Evaluated at")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /details$/ })).toBeNull();
    view.unmount();

    for (const profile of ["standard", "essential"] as const) {
      resetApi();
      view = await showDecision(DECISION_EXCEPTION, profile);
      const facts = allFacts();
      expect(facts.length, `facts blocks in ${profile}`).toBeGreaterThan(0);
      for (const block of facts) {
        expect(block.getAttribute("data-presentation")).toBe("collapsed");
        expect(
          within(block as HTMLElement).getByRole("button", { name: /Facts consulted/ }),
        ).toHaveAttribute("aria-expanded", "false");
      }
      for (const name of names) expect(screen.queryByText(name)).toBeNull();
      expect(inputs()?.getAttribute("data-presentation")).toBe("collapsed");
      expect(
        within(inputs() as HTMLElement).getByRole("button", { name: /Recorded inputs/ }),
      ).toHaveAttribute("aria-expanded", "false");
      expect(within(inputs() as HTMLElement).queryByRole("group")).toBeNull();
      if (profile === "standard") {
        expect(screen.queryByText("Evaluated at")).toBeNull();
        expect(screen.queryByRole("button", { name: /^\+\d+ details?$/ })).toBeNull();
        const details = screen.getByRole("button", { name: /^Details/ });
        expect(details).toHaveAttribute("aria-expanded", "false");
        await userEvent.click(details);
        expect(screen.getByText("Evaluated at")).toBeInTheDocument();
      } else {
        expect(screen.queryByText("Evaluated at")).toBeNull();
        const more = screen.getByRole("button", { name: "+4 details" });
        await userEvent.click(more);
        expect(screen.getByText("Evaluated at")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Hide 4 details" })).toBeInTheDocument();
      }
      // Opening the collapsed facts brings them back: nothing is lost.
      const toggle = within(facts[0] as HTMLElement).getByRole("button", {
        name: /Facts consulted/,
      });
      await userEvent.click(toggle);
      expect(screen.getAllByText(names[0] as string).length).toBeGreaterThan(0);
      view.unmount();
    }
  });

  it(
    "the_active_policy_the_rules_and_the_locked_deny_pill_are_identical_in_every_profile",
    async () => {
      const texts: Record<string, Record<string, string>> = {};
      const text = (id: string) =>
        [...document.querySelectorAll(`[data-visibility-id="${id}"]`)]
          .map((el) => el.textContent ?? "")
          .join("|");
      for (const profile of PROFILE_IDS) {
        resetApi();
        const record: Record<string, string> = {};
        let view = await showOverview(OVERVIEW_ACTIVE, "rules", profile);
        await screen.findByRole("table");
        record["policy.active"] = text("policy.active");
        record["policy.rules"] = text("policy.rules");
        view.unmount();
        resetApi();
        view = await showEditor(profile);
        record["draft.rules"] = text("draft.rules");
        view.unmount();
        resetApi();
        view = await showDecision(DECISION_LOCKED, profile);
        record["decision.locked"] = text("decision.locked");
        view.unmount();
        texts[profile] = record;
      }
      const baseline = texts["detailed"] as Record<string, string>;
      for (const id of ["policy.active", "policy.rules", "draft.rules", "decision.locked"]) {
        expect(baseline[id]?.length, `${id} has text`).toBeGreaterThan(5);
      }
      expect(baseline["decision.locked"]).toContain("Locked Deny");
      expect(baseline["policy.rules"]).toContain("Locked Deny");
      for (const profile of PROFILE_IDS) {
        expect(texts[profile], `text in ${profile}`).toEqual(baseline);
      }
    },
    SLOW,
  );
});

// ---------------------------------------------------------------------------------------------
// The password and the requests
// ---------------------------------------------------------------------------------------------

describe("publication does not depend on the profile", () => {
  it("publishing_still_asks_for_the_password_in_essential", async () => {
    const user = userEvent.setup();
    route(PUBLISH, () => ({}));
    const view = await showPublish(PREVIEW_BLOCKING, "essential");
    for (const box of await screen.findAllByRole("checkbox")) await user.click(box);
    await user.click(await screen.findByRole("button", { name: "Publish version 4" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByLabelText("Password")).toBeInTheDocument();
    expect(calls.filter((call) => call.route === PUBLISH)).toHaveLength(0);
    await user.type(within(dialog).getByLabelText("Password"), "hunter2");
    await user.click(within(dialog).getByRole("button", { name: "Publish version 4" }));
    expect(await screen.findByText("Published policy version 4.")).toBeInTheDocument();
    const sent = calls.filter((call) => call.route === PUBLISH);
    expect(sent).toHaveLength(1);
    expect(sent[0]?.init.body).toMatchObject({
      revision: DRAFT_4.revision,
      preview_hash: PREVIEW_BLOCKING.preview_hash,
      password: "hunter2",
    });
    view.unmount();
  });

  it(
    "the_profile_never_changes_which_requests_are_made",
    async () => {
      const screens: { name: string; show: (profile: ProfileId) => Promise<View> }[] = [
        { name: "overview", show: (profile) => showOverview(OVERVIEW_ACTIVE, "rules", profile) },
        { name: "publication", show: (profile) => showPublish(PREVIEW_BLOCKING, profile) },
        { name: "decision", show: (profile) => showDecision(DECISION_EXCEPTION, profile) },
        { name: "simulator", show: (profile) => showSimulator(profile) },
      ];
      for (const entry of screens) {
        const made: Record<string, string[]> = {};
        for (const profile of ["detailed", "essential"] as const) {
          resetApi();
          const view = await entry.show(profile);
          made[profile] = calls
            .filter((call) => !call.route.includes("preferences"))
            .map((call) => `${call.route} ${JSON.stringify(call.init)}`)
            .sort();
          view.unmount();
        }
        expect(made["detailed"]?.length, `${entry.name} makes requests`).toBeGreaterThan(1);
        expect(made["essential"], `${entry.name} requests`).toEqual(made["detailed"]);
      }
    },
    SLOW,
  );
});
