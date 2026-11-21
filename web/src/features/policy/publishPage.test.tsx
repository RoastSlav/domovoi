import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { PolicyPreview } from "../../api/types";
import { retryAfterText, WRONG_PASSWORD_TEXT } from "../../design/messages";
import { findingKindMeta, lockChangeMeta, policyOutcomeMeta } from "../../design/tone";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import {
  DRAFT_4,
  OVERVIEW_ACTIVE,
  OVERVIEW_FRESH,
  PREVIEW_BLOCKING,
  PREVIEW_CLEAN,
  PREVIEW_INVALID,
} from "../../test/policyFixtures";
import { renderAt } from "../../test/render";
import { PublishPage } from "./PublishPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const OVERVIEW = "GET /api/v1/policy";
const DRAFT = "GET /api/v1/policy/draft";
const PREVIEW = "POST /api/v1/policy/preview";
const PUBLISH = "POST /api/v1/policy/draft/publish";
const URL = "/policy/draft/publish";

function must<T>(value: T | undefined): T {
  if (value === undefined) throw new Error("fixture is missing an entry");
  return value;
}

beforeEach(() => {
  resetApi();
});

function open(preview: PolicyPreview) {
  route(OVERVIEW, () => OVERVIEW_ACTIVE);
  route(DRAFT, () => DRAFT_4);
  route(PREVIEW, () => preview);
  renderAt(URL, URL, <PublishPage />);
}

function publishButton(name = "Publish version 4") {
  return screen.findByRole("button", { name });
}

async function reasonOf(button: HTMLElement) {
  const user = userEvent.setup();
  await user.hover(button);
  return (await screen.findByRole("tooltip")).textContent;
}

async function tickAll(user: ReturnType<typeof userEvent.setup>) {
  for (const box of await screen.findAllByRole("checkbox")) {
    await user.click(box);
  }
}

async function confirmPublish(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await publishButton());
  const dialog = await screen.findByRole("dialog");
  await user.type(within(dialog).getByLabelText("Password"), "hunter2");
  await user.click(within(dialog).getByRole("button", { name: "Publish version 4" }));
  return dialog;
}

function published() {
  return calls.filter((call) => call.route === PUBLISH);
}

test("publish_page_lists_blocking_findings_with_boxes_and_informational_notes_apart", async () => {
  open(PREVIEW_BLOCKING);
  const boxes = await screen.findAllByRole("checkbox");
  expect(boxes).toHaveLength(3);
  const blocking = PREVIEW_BLOCKING.report?.findings.filter((f) => f.requires_acknowledgement);
  for (const finding of blocking ?? []) {
    expect(screen.getByText(finding.note)).toBeInTheDocument();
  }
  const resolution = policyOutcomeMeta("deny").label;
  expect(
    screen.getByRole("checkbox", {
      name: new RegExp(`Acknowledge: ask-containers and plex-restart resolve to ${resolution}`),
    }),
  ).toBeInTheDocument();
  expect(
    screen.getAllByText(findingKindMeta("incomparable_conflict").label).length,
  ).toBeGreaterThan(0);
  const informational = PREVIEW_BLOCKING.report?.findings.filter(
    (f) => !f.requires_acknowledgement,
  );
  for (const finding of informational ?? []) {
    expect(screen.getByText(finding.note)).toBeInTheDocument();
    expect(boxes.some((box) => box.closest("label")?.textContent?.includes(finding.note))).toBe(
      false,
    );
  }
  expect(screen.getByText(/Version 4: 6 rules, 2 locked, 1 shadow/)).toBeInTheDocument();
});

test("publish_is_disabled_with_a_reason_until_every_conflict_is_acknowledged", async () => {
  const user = userEvent.setup();
  open(PREVIEW_BLOCKING);
  const button = await publishButton();
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(await reasonOf(button)).toBe("Acknowledge every conflict above first.");
  const boxes = await screen.findAllByRole("checkbox");
  await user.click(must(boxes[0]));
  await user.click(must(boxes[1]));
  expect(await publishButton()).toHaveAttribute("aria-disabled", "true");
  await user.click(must(boxes[2]));
  expect(await publishButton()).not.toHaveAttribute("aria-disabled", "true");
  expect(published()).toHaveLength(0);
});

test("publishing_asks_for_the_password_and_sends_exactly_the_blocking_keys_and_the_shown_hash", async () => {
  const user = userEvent.setup();
  route(PUBLISH, () => ({}));
  open(PREVIEW_BLOCKING);
  await tickAll(user);
  await user.click(await publishButton());
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/3 conflicts acknowledged, 1 locked-rule changes/)).toBeVisible();
  expect(published()).toHaveLength(0);
  await user.type(within(dialog).getByLabelText("Password"), "hunter2");
  await user.click(within(dialog).getByRole("button", { name: "Publish version 4" }));
  expect(await screen.findByText("Published policy version 4.")).toBeInTheDocument();
  expect(published()).toHaveLength(1);
  expect(published()[0]?.init.body).toEqual({
    revision: DRAFT_4.revision,
    preview_hash: PREVIEW_BLOCKING.preview_hash,
    acknowledged: [...PREVIEW_BLOCKING.blocking].sort(),
    password: "hunter2",
  });
  expect(screen.getByTestId("location")).toHaveTextContent("/policy");
  expect(screen.getByText("other page")).toBeInTheDocument();
});

test("a_clean_review_publishes_with_an_empty_acknowledgement_list", async () => {
  const user = userEvent.setup();
  route(PUBLISH, () => ({}));
  open(PREVIEW_CLEAN);
  expect(await screen.findByText("No conflicts found.")).toBeInTheDocument();
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  await confirmPublish(user);
  expect(await screen.findByText("Published policy version 4.")).toBeInTheDocument();
  expect(published()[0]?.init.body).toMatchObject({ acknowledged: [], password: "hunter2" });
});

test("publish_failures_read_in_the_hubs_words_inside_the_dialog_and_refetch_the_review", async () => {
  const user = userEvent.setup();
  open(PREVIEW_CLEAN);
  route(PUBLISH, () => failure(403, "forbidden", "wrong"));
  const dialog = await confirmPublish(user);
  expect(await within(dialog).findByText(WRONG_PASSWORD_TEXT)).toBeInTheDocument();
  route(PUBLISH, () => failure(429, "too_many_requests", "slow", { "Retry-After": "30" }));
  await user.click(within(dialog).getByRole("button", { name: "Publish version 4" }));
  expect(await within(dialog).findByText(retryAfterText(30))).toBeInTheDocument();
  route(PUBLISH, () =>
    failure(400, "invalid", "bad", undefined, [
      { path: "rules", message: "Rule plex-restart is not valid.", severity: "error" },
    ]),
  );
  await user.click(within(dialog).getByRole("button", { name: "Publish version 4" }));
  expect(await within(dialog).findByText("Rule plex-restart is not valid.")).toBeInTheDocument();
  const before = calls.filter((call) => call.route === PREVIEW).length;
  route(PUBLISH, () => failure(409, "conflict", "The review is out of date."));
  await user.click(within(dialog).getByRole("button", { name: "Publish version 4" }));
  expect(await within(dialog).findByText(/The review is out of date/)).toBeInTheDocument();
  await user.click(within(dialog).getByRole("button", { name: "Review it again" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(calls.filter((call) => call.route === PREVIEW).length).toBeGreaterThan(before);
});

test("a_changed_review_prunes_ticks_that_no_longer_apply", async () => {
  const user = userEvent.setup();
  open(PREVIEW_BLOCKING);
  await tickAll(user);
  const [kept, , dropped] = PREVIEW_BLOCKING.blocking;
  const findings = PREVIEW_BLOCKING.report?.findings ?? [];
  const next: PolicyPreview = {
    ...PREVIEW_BLOCKING,
    preview_hash: "f".repeat(64),
    blocking: [must(kept), "equal_scope:new-a:new-b"],
    report: {
      blocked: true,
      findings: [
        ...findings.filter((f) => f.key === kept),
        {
          ...must(findings[1]),
          key: "equal_scope:new-a:new-b",
          first: "new-a",
          second: "new-b",
          note: "new-a and new-b: equal scope conflict.",
        },
      ],
    },
  };
  route(PREVIEW, () => next);
  route(PUBLISH, () => failure(409, "conflict", "The review is out of date."));
  await confirmPublish(user);
  await user.click(await screen.findByRole("button", { name: "Review it again" }));
  expect(await screen.findByText("new-a and new-b: equal scope conflict.")).toBeInTheDocument();
  const boxes = screen.getAllByRole("checkbox");
  expect(boxes).toHaveLength(2);
  expect(boxes[0]).toBeChecked();
  expect(boxes[1]).not.toBeChecked();
  expect(dropped).toBeDefined();
  expect(await reasonOf(await publishButton())).toBe("Acknowledge every conflict above first.");
});

test("the_locked_changes_block_and_the_widening_warning_are_always_visible", async () => {
  open(PREVIEW_BLOCKING);
  expect(
    await screen.findByText(lockChangeMeta(must(PREVIEW_BLOCKING.locked_changes[0]).change).label),
  ).toBeInTheDocument();
  expect(screen.getByText("no-prune")).toBeInTheDocument();
  expect(screen.getByText(/changing, unlocking or removing it needs your password/i)).toBeVisible();
  expect(
    screen.getByText("2 recorded decisions would become Allow automatically"),
  ).toBeInTheDocument();
  expect(screen.getByText("Based on the 200 most recent recorded decisions")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "See each decision" })).toHaveAttribute(
    "href",
    "/policy/simulator?rules=draft",
  );
});

test("the_publish_page_for_invalid_rules_lists_them_and_cannot_publish", async () => {
  open(PREVIEW_INVALID);
  expect(await screen.findByText("Fix these rules before publishing")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "plex-restart" })).toHaveAttribute(
    "href",
    "/policy/draft?rule=plex-restart",
  );
  expect(screen.getByText(/Two rules share the id ask-containers/)).toBeInTheDocument();
  const button = await publishButton();
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(await reasonOf(button)).toBe("Fix the problems above first.");
  expect(published()).toHaveLength(0);
});

test("no_draft_to_publish_is_an_empty_state_with_a_link", async () => {
  route(OVERVIEW, () => OVERVIEW_FRESH);
  renderAt(URL, URL, <PublishPage />);
  expect(await screen.findByText("No draft to publish")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Go to the draft" })).toHaveAttribute(
    "href",
    "/policy/draft",
  );
  expect(calls.some((call) => call.route === PREVIEW)).toBe(false);
});
