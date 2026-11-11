import { ICON_NAMES } from "./icons";
import { TIMELINE_KINDS, timelineMeta } from "./timeline";
import { TONES } from "./tone";

describe("timeline", () => {
  it.each(TIMELINE_KINDS)("%s reads as words, with a tone and an icon", (kind) => {
    const meta = timelineMeta({ kind, summary: "pending → open" });
    expect(meta.phrase).not.toBe("");
    expect(meta.phrase, "a known kind never shows raw").not.toBe(kind);
    expect(TONES).toContain(meta.tone);
    expect(ICON_NAMES).toContain(meta.icon);
  });

  it("a status change reads as what it changed to, in that status's tone", () => {
    const to = (summary: string) => timelineMeta({ kind: "status.changed", summary });
    expect(to("pending → open")).toMatchObject({ phrase: "Confirmed", tone: "danger" });
    expect(to("open → acked")).toMatchObject({ phrase: "Acknowledged", tone: "neutral" });
    expect(to("open → muted").phrase).toBe("Muted");
    expect(to("acked → resolved")).toMatchObject({ phrase: "Resolved", tone: "success" });
    expect(to("open → acked").detail, "the phrase says it all").toBeUndefined();
  });

  it("a status change that does not parse keeps the hub's line", () => {
    expect(timelineMeta({ kind: "status.changed", summary: "something else" })).toMatchObject({
      phrase: "Status changed",
      detail: "something else",
    });
  });

  it("a verdict reads as what triage said", () => {
    expect(timelineMeta({ kind: "verdict", summary: "ACTION NEEDED" })).toMatchObject({
      phrase: "Triage said Action needed",
      tone: "danger",
    });
    expect(timelineMeta({ kind: "verdict", summary: "FIXED" }).tone).toBe("success");
  });

  it("an action step takes its words and tone from the action's status", () => {
    expect(timelineMeta({ kind: "action.verify_failed", summary: "restart" })).toMatchObject({
      phrase: "Action: Not fixed",
      tone: "danger",
      detail: "restart",
    });
  });

  it("a follow-up question keeps its text out of the timeline", () => {
    const meta = timelineMeta({ kind: "triage.followup", summary: "Was it the backup?" });
    expect(meta.phrase).toBe("You asked a follow-up");
    expect(meta.detail).toBeUndefined();
  });

  it("an unknown kind shows as it is, with the hub's line", () => {
    expect(timelineMeta({ kind: "from.a_newer_hub", summary: "Something new" })).toEqual({
      phrase: "from.a_newer_hub",
      tone: "neutral",
      icon: "dot",
      detail: "Something new",
    });
    expect(timelineMeta({ kind: "toString", summary: "" }).phrase).toBe("toString");
  });
});
