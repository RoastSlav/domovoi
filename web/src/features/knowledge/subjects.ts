import type { SubjectKind } from "../../design/knowledgeText";

/** A subject reference taken apart. Ids are opaque: never assume one is a UUID. */
export interface ParsedSubject {
  /** For a container or unit: the id of its host; for a host: its own id. */
  host?: string;
  /** For an entity: its id. */
  id?: string;
  kind: SubjectKind;
  /** For a container or unit: its name. */
  name?: string;
}

/** The host and entity names a page has loaded, so references can be shown in words. */
export interface SubjectNames {
  entities: ReadonlyMap<string, string>;
  hosts: ReadonlyMap<string, string>;
}

/** Takes `<kind>:<rest>` apart; a container or unit splits `<rest>` at the first `/`. */
export function parseSubject(ref: string): ParsedSubject | undefined {
  const colon = ref.indexOf(":");
  if (colon <= 0) {
    return undefined;
  }
  const kind = ref.slice(0, colon);
  const rest = ref.slice(colon + 1);
  if (rest === "") {
    return undefined;
  }
  switch (kind) {
    case "host":
      return { kind, host: rest };
    case "entity":
      return { kind, id: rest };
    case "container":
    case "unit": {
      const slash = rest.indexOf("/");
      if (slash <= 0 || slash === rest.length - 1) {
        return undefined;
      }
      return { kind, host: rest.slice(0, slash), name: rest.slice(slash + 1) };
    }
    default:
      return undefined;
  }
}

/** The reference for a parsed subject: the inverse of `parseSubject`. */
export function formatSubject(subject: ParsedSubject): string {
  switch (subject.kind) {
    case "host":
      return `host:${subject.host ?? ""}`;
    case "entity":
      return `entity:${subject.id ?? ""}`;
    default:
      return `${subject.kind}:${subject.host ?? ""}/${subject.name ?? ""}`;
  }
}

function hostPart(hostId: string | undefined, names: SubjectNames): { text: string; to?: string } {
  const name = hostId === undefined ? undefined : names.hosts.get(hostId);
  return name === undefined
    ? { text: "an unknown host" }
    : { text: name, to: `/hosts/${encodeURIComponent(name)}` };
}

/** A subject in words, with where it links, if anywhere. The reference is shown as data, never as markup. */
export function subjectLabel(ref: string, names: SubjectNames): { text: string; to?: string } {
  const subject = parseSubject(ref);
  if (subject === undefined) {
    return { text: ref };
  }
  if (subject.kind === "host") {
    return hostPart(subject.host, names);
  }
  if (subject.kind === "entity") {
    const name = names.entities.get(subject.id ?? "");
    return name === undefined
      ? { text: "a removed entity" }
      : { text: name, to: `/knowledge/entities/${encodeURIComponent(subject.id ?? "")}` };
  }
  const host = hostPart(subject.host, names);
  const text = `${subject.name ?? ""} on ${host.text}`;
  return host.to === undefined ? { text } : { text, to: host.to };
}
