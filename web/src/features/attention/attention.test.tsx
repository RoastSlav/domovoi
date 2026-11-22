import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { failure, resetApi, route } from "../../test/apiMock";
import { hubHost, incompatibleHost, offlineHost, summary } from "../../test/attentionFixtures";
import { serveAttention } from "../../test/attentionRoutes";
import { incident, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { AttentionPage } from "./AttentionPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  localStorage.clear();
});

const open = (url = "/attention") => renderAt(url, "/attention", <AttentionPage />);
const region = (name: string) => within(screen.getByRole("region", { name }));

describe("attention page", () => {
  it("attention_lists_tiers_in_order_with_their_headings", async () => {
    serveAttention({
      hosts: [nas, incompatibleHost("old")],
      incidents: [incident({ id: "i-warn", title: "Slow disk", severity: "warning" })],
      operations: { paused: [summary("p", "paused", { pause_reason: { type: "error" } })] },
    });
    open();

    await screen.findByText("Slow disk");
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings).toEqual(["Act now", "Waiting for you", "Worth a look"]);
    expect(region("Act now").getByRole("link", { name: /old/ })).toBeVisible();
    expect(region("Waiting for you").getByText("It paused after an error.")).toBeVisible();
    expect(region("Worth a look").getByRole("link", { name: /Slow disk/ })).toBeVisible();
  });

  it("a_result_unknown_operation_is_a_link_row_with_its_why_and_goes_to_the_operation", async () => {
    serveAttention({
      operations: {
        paused: [summary("op-9", "paused", { pause_reason: { type: "outcome_unknown" } })],
      },
    });
    open();

    const link = await screen.findByRole("link", { name: /Goal op-9/ });
    expect(link).toHaveAttribute("href", "/operations/op-9");
    expect(link).toHaveTextContent("Result unknown");
    expect(link).toHaveTextContent(
      "It may have run on the host. Check the host before continuing.",
    );
    expect(link).toHaveTextContent("nas");
  });

  it("an_offline_host_and_an_incompatible_one_are_told_apart_and_a_hub_only_host_is_left_out", async () => {
    serveAttention({ hosts: [offlineHost("down"), incompatibleHost("old"), hubHost("home")] });
    open();

    await screen.findByRole("region", { name: "Act now" });
    const act = region("Act now");
    expect(act.getByRole("link", { name: /Incompatible.*old/ })).toHaveAttribute(
      "href",
      "/hosts/old",
    );
    expect(act.getByRole("link", { name: /Offline.*down/ })).toBeVisible();
    expect(screen.queryByRole("link", { name: /home/ })).toBeNull();
  });

  it("all_clear_shows_nothing_needs_you_only_when_every_source_loaded", async () => {
    serveAttention();
    open();

    expect(await screen.findByText("Nothing needs you")).toBeVisible();
  });

  it("a_failed_source_is_named_and_the_rest_is_listed_never_nothing_needs_you", async () => {
    serveAttention({
      operations: { waiting_user: [summary("q", "waiting_user")] },
    });
    route("GET /api/v1/operations", (init) =>
      init.params?.query?.status === "paused"
        ? failure(500, "internal", "boom")
        : {
            items:
              init.params?.query?.status === "waiting_user" ? [summary("q", "waiting_user")] : [],
            next_cursor: null,
            as_of_seq: 0,
          },
    );
    open();

    expect(await screen.findByText("Could not load paused operations")).toBeVisible();
    expect(await screen.findByRole("link", { name: /Goal q/ })).toBeVisible();
    expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
    expect(screen.queryByText("Nothing needs you")).toBeNull();
  });

  it("retry_refetches_every_failed_source", async () => {
    const { default: userEvent } = await import("@testing-library/user-event");
    const { calls } = await import("../../test/apiMock");
    serveAttention();
    route("GET /api/v1/incidents", () => failure(500, "internal", "boom"));
    route("GET /api/v1/hosts", () => failure(500, "internal", "boom"));
    open();

    expect(await screen.findByText("Could not load incidents and hosts")).toBeVisible();
    const before = (key: string) => calls.filter((call) => call.route === key).length;
    const incidents = before("GET /api/v1/incidents");
    const hosts = before("GET /api/v1/hosts");
    await userEvent.setup().click(screen.getByRole("button", { name: "Retry" }));

    await vi.waitFor(() => {
      expect(before("GET /api/v1/incidents")).toBeGreaterThan(incidents);
      expect(before("GET /api/v1/hosts")).toBeGreaterThan(hosts);
    });
  });

  it("more_than_one_page_says_so", async () => {
    serveAttention({ operations: { paused: [summary("p", "paused")] }, more: ["paused"] });
    open();

    expect(await screen.findByText(/More items exist than are listed here/)).toBeVisible();
  });

  it("attention_page_has_no_approve_or_deny_outside_a_card", async () => {
    serveAttention({
      operations: {
        paused: [summary("p", "paused")],
        waiting_user: [summary("q", "waiting_user")],
      },
      incidents: [incident()],
      hosts: [nas, offlineHost("down")],
    });
    open();

    await screen.findByRole("link", { name: /Goal p/ });
    expect(screen.queryByRole("button", { name: /^(Approve|Deny)$/ })).toBeNull();
    expect(screen.queryByRole("group", { name: /^Action / })).toBeNull();
  });

  it("the_attention_page_follows_the_host_selector", async () => {
    const { calls } = await import("../../test/apiMock");
    serveAttention();
    open("/attention?host=nas");

    await screen.findByText("Nothing needs you");
    const asked = calls.filter((call) => /\/(incidents|actions|operations)$/.test(call.route));
    expect(asked.length).toBeGreaterThanOrEqual(6);
    for (const call of asked) {
      expect(call.init.params?.query, call.route).toMatchObject({ host: "nas" });
    }
  });
});
