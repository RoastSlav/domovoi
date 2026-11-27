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

function isIpv4(text: string): boolean {
  const octets = text.split(".");
  return (
    octets.length === 4 &&
    octets.every((octet) => /^(0|[1-9]\d{0,2})$/.test(octet) && Number(octet) <= 255)
  );
}

/** The groups one side of `::` stands for (an IPv4 tail counts as two), or -1 when malformed. */
function ipv6Groups(side: string): number {
  if (side === "") {
    return 0;
  }
  const words = side.split(":");
  let groups = 0;
  for (const [index, word] of words.entries()) {
    if (index === words.length - 1 && isIpv4(word)) {
      groups += 2;
    } else if (/^[0-9a-f]{1,4}$/i.test(word)) {
      groups += 1;
    } else {
      return -1;
    }
  }
  return groups;
}

function isIpv6(text: string): boolean {
  const sides = text.split("::");
  const counts = sides.map(ipv6Groups);
  if (sides.length > 2 || counts.includes(-1)) {
    return false;
  }
  const total = counts.reduce((sum, n) => sum + n, 0);
  // `::` stands for at least one group; an IPv4 tail may only end the whole address.
  return sides.length === 2 ? total <= 7 && !sides[0]?.includes(".") : total === 8;
}

/**
 * Whether the hub will read `text` as a network: an IPv4 or IPv6 address with an optional
 * prefix length. It mirrors the hub's parser so a typo is said before the password prompt;
 * the hub still decides.
 */
export function isNetwork(text: string): boolean {
  const [address = "", prefix, ...rest] = text.split("/");
  if (rest.length > 0) {
    return false;
  }
  const host = address.trim();
  const v6 = host.includes(":");
  if (v6 ? !isIpv6(host) : !isIpv4(host)) {
    return false;
  }
  // An IPv4-mapped IPv6 address is read as the IPv4 address it maps.
  const mapped = v6 && /^(::|(0{1,4}:){5})ffff:[^:]*(\.|:)[^:]*$/i.test(host);
  const width = v6 && !mapped ? 128 : 32;
  return prefix === undefined || (/^\+?\d+$/.test(prefix) && Number(prefix) <= width);
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
      if (!isNetwork(cidr)) {
        return bad("That is not a network. Write one such as 192.168.1.0/24 or fd00::/64.");
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
