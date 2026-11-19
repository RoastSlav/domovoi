import { QueryClient } from "@tanstack/react-query";
import fixture from "../test/fixtures/journal-frames/frames.json";
import { applyJournal, parseJournalFrame, type JournalFrame } from "./journalFrames";

const INCIDENT = "0190a0a0-0000-7000-8000-0000000000a1";
const journal = fixture.frames.filter((f) => f.event === "journal");
const V1_NAMES = [
  "incident_opened",
  "incident_folded",
  "incident_escalated",
  "incident_reopened",
  "incident_resolved",
  "incident_acked",
  "incident_muted",
  "incident_unmuted",
  "triage_completed",
  "action_changed",
  "action_blocked",
];

function fixtureFrame(name: string): JournalFrame {
  const parsed = parseJournalFrame(journal.find((f) => f.name === name)?.data);
  if (!parsed) {
    throw new Error(`no fixture frame ${name}`);
  }
  return parsed;
}

function spied() {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const keysOf = () => invalidate.mock.calls.map((args) => args[0]?.queryKey);
  return { client, invalidate, keysOf };
}

function raw(seq: number, type = "incident"): JournalFrame {
  return { ...fixtureFrame("incident_opened"), aggregate_type: type, seq };
}

test("fixture_frames_parse", () => {
  expect(journal.length).toBeGreaterThanOrEqual(12);
  for (const entry of journal) {
    expect(parseJournalFrame(entry.data), entry.name).toBeDefined();
    expect(parseJournalFrame(JSON.stringify(entry.data)), entry.name).toBeDefined();
  }
  expect(parseJournalFrame("{not json")).toBeUndefined();
  expect(parseJournalFrame({ seq: 1 })).toBeUndefined();
});

test("journal_frames_cover_every_v1_event", async () => {
  for (const name of V1_NAMES) {
    const { client, keysOf } = spied();
    const frame = fixtureFrame(name);
    await applyJournal(client, frame);
    const keys = keysOf();
    expect(keys, name).toContainEqual(["incidents", "list"]);
    expect(keys, name).toContainEqual(["incidents", "detail", INCIDENT]);
    expect(keys, name).toContainEqual(["overview"]);
    if (frame.aggregate_type === "action") {
      expect(keys, name).toContainEqual(["actions"]);
    }
  }
});

test("operation_frames_invalidate_operations", async () => {
  const { client, keysOf } = spied();
  await applyJournal(client, fixtureFrame("operation_step_finished"));
  expect(keysOf()).toEqual([
    ["operations"],
    ["operations", "detail", "0190a0a0-0000-7000-8000-0000000000b1"],
    ["overview"],
  ]);
});

test("unknown_aggregate_types_and_kinds_are_ignored", async () => {
  const { client, invalidate } = spied();
  await applyJournal(client, { ...raw(30, "fact"), kind: "fact.something_new" });
  await applyJournal(client, { ...raw(31, "brand_new"), kind: "x", payload: { a: 1 } });
  expect(invalidate).not.toHaveBeenCalled();
});

test("a_snapshot_newer_than_the_event_is_not_refetched", async () => {
  const { client } = spied();
  client.setQueryData(["incidents", "list", {}], { as_of_seq: 12 });
  const state = () => client.getQueryState(["incidents", "list", {}])?.isInvalidated;
  await applyJournal(client, raw(12));
  expect(state()).toBe(false);
  await applyJournal(client, raw(9));
  expect(state()).toBe(false);
  await applyJournal(client, raw(13));
  expect(state()).toBe(true);
});
