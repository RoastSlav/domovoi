import { describe, expect, it } from "vitest";
import type { Host, IncidentSummary } from "../../api/types";
import {
  hubHost,
  incompatibleHost,
  neverConnectedHost,
  offlineHost,
  summary,
} from "../../test/attentionFixtures";
import { incident, nas } from "../../test/fixtures";
import { attentionItems, type AttentionInputs } from "./attentionModel";

const NAMES = new Map([[nas.id, "nas"]]);

function inputs(parts: Partial<AttentionInputs> = {}): AttentionInputs {
  return {
    incidents: [],
    waitingApproval: [],
    waitingUser: [],
    paused: [],
    hosts: [],
    hostNames: NAMES,
    ...parts,
  };
}

const expectedIncompatible = (name: string): Host =>
  offlineHost(name, { protocol_state: "expected_incompatible" });
const keys = (parts: Partial<AttentionInputs>) => attentionItems(inputs(parts)).map((i) => i.key);
const sick = (id: string, severity: string, at: string): IncidentSummary =>
  incident({ id, severity, last_firing_at: at });

describe("attention ranking", () => {
  it("ranking_follows_the_fixed_table", () => {
    // One input per rank 0-10, deliberately handed over out of order.
    const items = attentionItems(
      inputs({
        incidents: [
          sick("i10", "info", "2026-01-01T00:00:00Z"),
          sick("i9", "warning", "2026-01-01T00:00:00Z"),
          sick("i3", "critical", "2026-01-01T00:00:00Z"),
        ],
        hosts: [expectedIncompatible("h8"), offlineHost("h4"), incompatibleHost("h2")],
        paused: [
          summary("o7", "paused", { pause_reason: { type: "error" } }),
          summary("o0", "paused", { pause_reason: { type: "outcome_unknown" } }),
        ],
        waitingUser: [summary("o6", "waiting_user")],
        waitingApproval: [summary("o5", "waiting_approval")],
      }),
    );

    expect(items.map((item) => [item.key, item.rank, item.tier])).toEqual([
      ["operation:o0", 0, 0],
      ["host:host-h2", 2, 0],
      ["incident:i3", 3, 0],
      ["host:host-h4", 4, 0],
      ["operation:o5", 5, 1],
      ["operation:o6", 6, 1],
      ["operation:o7", 7, 1],
      ["host:host-h8", 8, 1],
      ["incident:i9", 9, 2],
      ["incident:i10", 10, 2],
    ]);
  });

  it("a_result_unknown_operation_comes_before_everything_else", () => {
    const items = attentionItems(
      inputs({
        incidents: [sick("crit", "critical", "2026-01-01T00:00:00Z")],
        hosts: [incompatibleHost("bad")],
        waitingApproval: [summary("wait", "waiting_approval")],
        paused: [summary("unk", "paused", { pause_reason: { type: "outcome_unknown" } })],
      }),
    );

    expect(items[0]?.key).toBe("operation:unk");
    expect(items[0]?.meta.label).toBe("Result unknown");
    expect(items[0]?.why).toBe("It may have run on the host. Check the host before continuing.");
  });

  it("an_incompatible_host_is_its_own_row_ahead_of_an_offline_one_and_never_reads_as_offline", () => {
    const items = attentionItems(
      inputs({
        hosts: [offlineHost("down"), incompatibleHost("old"), expectedIncompatible("fresh")],
      }),
    );

    expect(items.map((item) => [item.title, item.meta.label, item.rank])).toEqual([
      ["old", "Incompatible", 2],
      ["down", "Offline", 4],
      ["fresh", "Expected incompatible", 8],
    ]);
    expect(items[0]?.href).toBe("/hosts/old");
    expect(items[2]?.tier).toBe(1);
  });

  it("a_host_that_never_connected_or_is_the_hub_itself_is_not_an_attention_item", () => {
    expect(
      keys({ hosts: [neverConnectedHost("new"), hubHost("home"), { ...nas }] }),
      "never connected, the hub's own machine and a connected host",
    ).toEqual([]);
  });

  it("critical_incidents_are_act_now_and_warning_and_info_are_worth_a_look", () => {
    const items = attentionItems(
      inputs({
        incidents: [
          sick("i", "info", "2026-01-01T00:00:00Z"),
          sick("w", "warning", "2026-01-01T00:00:00Z"),
          sick("c", "critical", "2026-01-01T00:00:00Z"),
          sick("u", "mystery", "2026-01-01T00:00:00Z"),
        ],
      }),
    );

    expect(items.map((item) => [item.key, item.tier, item.rank, item.meta.label])).toEqual([
      ["incident:c", 0, 3, "Critical"],
      ["incident:w", 2, 9, "Warning"],
      ["incident:i", 2, 10, "Info"],
      ["incident:u", 2, 10, "mystery"],
    ]);
  });

  it("approvals_list_the_oldest_waiting_operation_first", () => {
    const items = attentionItems(
      inputs({
        waitingApproval: [
          summary("new", "waiting_approval", { updated_at: "2026-01-02T12:00:00Z" }),
          summary("old", "waiting_approval", { updated_at: "2026-01-02T09:00:00Z" }),
        ],
      }),
    );

    expect(items.map((item) => item.key)).toEqual(["operation:old", "operation:new"]);
  });

  it("other_ranks_order_by_recency_with_unknown_times_last_and_keys_as_the_tie_break", () => {
    const order = keys({
      incidents: [
        sick("d", "warning", "not a time"),
        sick("c", "warning", "2026-01-01T10:00:00Z"),
        sick("b", "warning", "2026-01-01T12:00:00Z"),
        sick("a", "warning", "2026-01-01T12:00:00Z"),
      ],
    });

    expect(order).toEqual(["incident:a", "incident:b", "incident:c", "incident:d"]);
  });

  it("an_incident_and_an_approval_about_it_are_two_items", () => {
    const items = attentionItems(
      inputs({
        incidents: [sick("inc-1", "critical", "2026-01-01T00:00:00Z")],
        waitingApproval: [summary("op", "waiting_approval", { incidents: ["inc-1"] })],
      }),
    );

    expect(items.map((item) => [item.kind, item.key])).toEqual([
      ["incident", "incident:inc-1"],
      ["approval", "operation:op"],
    ]);
  });

  it("paused_operations_say_why_in_plain_words", () => {
    const why = (reason: { type: string } | null) =>
      attentionItems(inputs({ paused: [summary("p", "paused", { pause_reason: reason })] }))[0]
        ?.why;

    expect(why({ type: "budget" })).toBe("A limit is spent. Raise it to continue, or cancel it.");
    expect(why({ type: "error" })).toBe("It paused after an error.");
    expect(why({ type: "manual" })).toBe("It was paused by a person.");
    expect(why({ type: "global_pause" })).toBe("It is paused.");
    expect(why({ type: "something_new" })).toBe("It is paused.");
    expect(why(null)).toBe("It is paused.");
  });

  it("every_pill_comes_from_a_design_lookup_and_unknown_values_are_neutral", () => {
    const items = attentionItems(
      inputs({
        incidents: [sick("odd", "mystery", "2026-01-01T00:00:00Z")],
        waitingApproval: [summary("a", "waiting_approval")],
        waitingUser: [summary("q", "waiting_user")],
        paused: [summary("p", "paused")],
      }),
    );
    const label = (key: string) => items.find((item) => item.key === key)?.meta;

    expect(label("operation:a")?.label).toBe("Waiting for approval");
    expect(label("operation:q")?.label).toBe("Question");
    expect(label("operation:p")?.label).toBe("Paused");
    expect(label("incident:odd")).toMatchObject({ label: "mystery", tone: "neutral" });
  });

  it("a_question_row_shows_the_question_and_says_when_it_carries_on", () => {
    const asked = {
      ...summary("q", "waiting_user"),
      question: {
        step: "s1",
        text: "Is /srv meant to be full?",
        expires_at: "2026-11-01T14:05:00Z",
      },
    };
    const [row] = attentionItems(inputs({ waitingUser: [asked] }));
    expect(row?.title).toBe("Question: Is /srv meant to be full?");
    expect(row?.why).toMatch(
      /Triage asked this and is waiting for your answer\. It carries on at /,
    );
  });
});
