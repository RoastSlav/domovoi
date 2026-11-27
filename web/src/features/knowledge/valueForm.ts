import { predicateInfo } from "../../design/knowledgeText";

/** What the add-a-fact form holds: one field set per predicate; only the relevant ones are read. */
export interface ValueDraft {
  cidr: string;
  criticality: string;
  dependsOn: string;
  endsAt: string;
  note: string;
  predicate: string;
  startsAt: string;
  text: string;
}

/** The value to send, or one short sentence saying what is wrong with the input. */
export type BuiltValue = { ok: true; value: unknown } | { ok: false; problem: string };

const LOCAL_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/;

function bad(problem: string): BuiltValue {
  return { ok: false, problem };
}

function text(raw: string, max: number): BuiltValue {
  const value = raw.trim();
  const length = Array.from(value).length;
  return length >= 1 && length <= max ? { ok: true, value } : bad(`Write 1 to ${max} characters.`);
}

function window(draft: ValueDraft): BuiltValue {
  if (!LOCAL_TIME.test(draft.startsAt) || !LOCAL_TIME.test(draft.endsAt)) {
    return bad("Choose a start and an end.");
  }
  const starts_at = `${draft.startsAt}:00Z`;
  const ends_at = `${draft.endsAt}:00Z`;
  const start = Date.parse(starts_at);
  const end = Date.parse(ends_at);
  if (Number.isNaN(start) || Number.isNaN(end)) {
    return bad("Choose a start and an end.");
  }
  if (end <= start) {
    return bad("The end must be after the start.");
  }
  const note = draft.note.trim();
  if (note === "") {
    return { ok: true, value: { starts_at, ends_at } };
  }
  return Array.from(note).length > 200
    ? bad("Write 1 to 200 characters.")
    : { ok: true, value: { starts_at, ends_at, note } };
}

/**
 * Builds the value of a fact from the form, or says what is wrong. A cidr is sent as typed:
 * the hub writes it in its standard form and the page then shows what it stored.
 */
export function buildValue(draft: ValueDraft): BuiltValue {
  if (!predicateInfo(draft.predicate).assertable) {
    return bad("This kind of fact cannot be entered by hand.");
  }
  switch (draft.predicate) {
    case "role":
      return text(draft.text, 80);
    case "owner_note":
      return text(draft.text, 1000);
    case "backup_location":
      return text(draft.text, 200);
    case "criticality":
      return draft.criticality === "critical" || draft.criticality === "warning"
        ? { ok: true, value: draft.criticality }
        : bad("Choose critical or warning.");
    case "never_restart":
      return { ok: true, value: true };
    case "depends_on":
      return draft.dependsOn.trim() === ""
        ? bad("Choose what it depends on.")
        : { ok: true, value: draft.dependsOn };
    case "management_path":
    case "trusted_cidr": {
      const cidr = draft.cidr.trim();
      if (cidr === "") {
        return bad("Enter a network, for example 192.168.1.0/24.");
      }
      if (/^[^/]+\/0$/.test(cidr)) {
        return bad(
          "That network is every address (/0). Name the network you really manage, for example 192.168.1.0/24.",
        );
      }
      return { ok: true, value: { cidr } };
    }
    case "maintenance_window":
      return window(draft);
    default:
      return bad("This kind of fact cannot be entered by hand.");
  }
}
