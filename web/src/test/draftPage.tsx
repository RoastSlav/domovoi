import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Link } from "react-router";
import type {
  PolicyDraft,
  PolicyDraftSaved,
  PolicyOverview,
  PolicyPreview,
  PolicyRule,
} from "../api/types";
import { DraftPage } from "../features/policy/DraftPage";
import { calls, resetApi, route } from "./apiMock";
import { nas } from "./fixtures";
import { DRAFT_4, OVERVIEW_ACTIVE, PREVIEW_CLEAN } from "./policyFixtures";
import { renderAt } from "./render";

/** The routes the draft page talks to. */
export const OVERVIEW = "GET /api/v1/policy";
export const DRAFT = "GET /api/v1/policy/draft";
export const PREVIEW = "POST /api/v1/policy/preview";
export const SAVE = "PUT /api/v1/policy/draft";
export const DISCARD = "DELETE /api/v1/policy/draft";
export const HOSTS = "GET /api/v1/hosts";

/** What the fake hub currently stores as the draft, and what it answers to a check. */
export const hub: { draft: PolicyDraft; preview: PolicyPreview } = {
  draft: DRAFT_4,
  preview: PREVIEW_CLEAN,
};

/** The user driver every helper takes. */
export type User = ReturnType<typeof userEvent.setup>;

/** Renders the draft page against a fake hub that stores what is saved. */
export function setup(overview: PolicyOverview = OVERVIEW_ACTIVE, url = "/policy/draft") {
  resetApi();
  hub.draft = DRAFT_4;
  hub.preview = PREVIEW_CLEAN;
  route(OVERVIEW, () => overview);
  route(DRAFT, () => hub.draft);
  route(HOSTS, () => [{ ...nas, id: "0192a000-0000-7000-8000-000000000001" }]);
  route(PREVIEW, () => hub.preview);
  route(SAVE, (init) => {
    const body = init.body as { rules: PolicyRule[] };
    hub.draft = { ...hub.draft, revision: hub.draft.revision + 1, rules: body.rules };
    const saved: PolicyDraftSaved = {
      draft: hub.draft,
      locked_changes: hub.preview.locked_changes,
    };
    return saved;
  });
  route(DISCARD, () => null);
  return renderAt(
    url,
    "/policy/draft",
    <>
      <DraftPage />
      <Link to="/policy">Leave the page</Link>
    </>,
  );
}

/** Opens the Edit item of a rule's row menu. */
export async function editRule(user: User, id: string) {
  await user.click(await screen.findByRole("button", { name: `Actions for ${id}` }));
  await user.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: "Edit" }));
}

/** Changes the note of a rule and applies it to the draft. */
export async function changeNote(user: User, id: string, note: string) {
  await editRule(user, id);
  const field = screen.getByRole("textbox", { name: "Note" });
  await user.clear(field);
  await user.type(field, note);
  await user.click(screen.getByRole("button", { name: "Apply to draft" }));
}

/** The requests of one route made so far. */
export function made(key: string) {
  return calls.filter((call) => call.route === key);
}
