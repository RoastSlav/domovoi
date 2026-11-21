import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { calls, resetApi } from "../../test/apiMock";
import { FACTS_PAGE } from "../../test/knowledgeFixtures";
import {
  OPERATION_WITH_SUMMARY,
  PATHS_AVAILABLE,
  PATHS_PAGE,
  factScreen,
  serveScreens,
} from "../../test/knowledgeScreens";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  serveScreens();
});

const SLOW = 40_000;
const fact = (id: string) => {
  const found = FACTS_PAGE.items.find((candidate) => candidate.id === id);
  if (!found) {
    throw new Error(`no fixture fact ${id}`);
  }
  return found;
};
const block = (id: string) => document.querySelector(`[data-visibility-id="${id}"]`);
const toggle = (label: RegExp | string) => screen.queryByRole("button", { name: label });

describe("standard_collapses_origin_history_and_similar_operations_and_essential_hides_the_times", () => {
  it(
    "folds origin, history and times behind toggles in Standard",
    async () => {
      const user = userEvent.setup();
      await factScreen(fact("fact-role")).render("standard");

      const origin = toggle(/Where it came from/);
      expect(origin).toHaveAttribute("aria-expanded", "false");
      expect(toggle(/^History/)).toHaveAttribute("aria-expanded", "false");
      expect(toggle(/^Times/)).toHaveAttribute("aria-expanded", "false");
      expect(screen.queryByText("Recorded by")).toBeNull();
      await user.click(origin as HTMLElement);
      expect(await screen.findByText("Recorded by")).toBeVisible();
    },
    SLOW,
  );

  it(
    "hides the times behind a plus-five-details button in Essential",
    async () => {
      const user = userEvent.setup();
      await factScreen(fact("fact-role")).render("essential");

      expect(toggle(/^Times/)).toBeNull();
      expect(screen.queryByText("Last verified")).toBeNull();
      await user.click(screen.getByRole("button", { name: "+5 details" }));
      expect(await screen.findByText("Last verified")).toBeVisible();
      expect(toggle(/Where it came from/)).toHaveAttribute("aria-expanded", "false");
    },
    SLOW,
  );

  it(
    "shows every block in place in Detailed",
    async () => {
      await factScreen(fact("fact-role")).render("detailed");
      expect(block("fact.origin")?.getAttribute("data-presentation")).toBe("shown");
      expect(block("fact.history")?.getAttribute("data-presentation")).toBe("shown");
      expect(screen.getByText("Last verified")).toBeVisible();
    },
    SLOW,
  );

  it(
    "folds the similar past operations behind a toggle in Standard and Essential",
    async () => {
      for (const profile of ["standard", "essential"] as const) {
        const view = await OPERATION_WITH_SUMMARY.render(profile);
        expect(toggle(/Similar past operations/)).toHaveAttribute("aria-expanded", "false");
        view.unmount();
      }
    },
    SLOW,
  );
});

describe("approve_like_controls_are_available_in_every_profile", () => {
  it(
    "offers Accept, Retract and the password prompt in Essential",
    async () => {
      const user = userEvent.setup();
      await factScreen(fact("fact-inferred-cidr")).render("essential");

      expect(screen.getByRole("button", { name: "Make it mine" })).toBeEnabled();
      expect(screen.getByRole("button", { name: "Retract" })).toBeEnabled();
      await user.click(screen.getByRole("button", { name: "Make it mine" }));
      const prompt = await screen.findByRole("dialog", { name: "Confirm with your password" });
      expect(within(prompt).getByLabelText("Password")).toBeVisible();
    },
    SLOW,
  );

  it(
    "offers Confirm disabled with its reason, and Accept and Retract, in Essential",
    async () => {
      const user = userEvent.setup();
      await PATHS_PAGE.render("essential");

      const confirm = screen.getAllByRole("button", { name: /^Confirm/ })[0] as HTMLElement;
      expect(confirm).toHaveAttribute("aria-disabled", "true");
      await user.hover(confirm);
      expect(await screen.findByText(/Coverage cannot be verified yet/)).toBeVisible();
      expect(screen.getAllByRole("button", { name: "Retract" }).length).toBeGreaterThan(0);
      await user.click(screen.getByRole("button", { name: "Accept as mine" }));
      expect(
        await screen.findByRole("dialog", { name: "Confirm with your password" }),
      ).toBeVisible();
    },
    SLOW,
  );
});

describe("the_profile_never_changes_which_requests_are_made_for_p0_content", () => {
  /** The distinct requests a screen made, as `METHOD path` plus its query. */
  async function requestsAt(profile: "detailed" | "essential", paths = false) {
    resetApi();
    serveScreens(paths ? PATHS_AVAILABLE : undefined);
    const view = await PATHS_PAGE.render(profile);
    await waitFor(() => expect(block("paths.list")).not.toBeNull());
    const made = [
      ...new Set(
        calls
          .filter((call) => !call.route.includes("/me/preferences"))
          .map((call) => `${call.route} ${JSON.stringify(call.init.params ?? {})}`),
      ),
    ].sort();
    view.unmount();
    return made;
  }

  it(
    "makes the same paths-page requests in Detailed and Essential",
    async () => {
      const detailed = await requestsAt("detailed");
      const essential = await requestsAt("essential");
      expect(detailed.length).toBeGreaterThan(1);
      expect(essential).toEqual(detailed);
    },
    SLOW,
  );

  it(
    "makes the same paths-page requests when coverage can be checked",
    async () => {
      expect(await requestsAt("essential", true)).toEqual(await requestsAt("detailed", true));
    },
    SLOW,
  );
});
