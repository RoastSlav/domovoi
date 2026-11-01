import type { components } from "./schema";

type Schemas = components["schemas"];

/** The logged-in user and the session's CSRF token. */
export type Session = Schemas["SessionDto"];
/** One incident in a list. */
export type IncidentSummary = Schemas["IncidentSummary"];
/** One page of incidents. */
export type IncidentPage = Schemas["IncidentPage"];
/** Everything about one incident. */
export type IncidentDetail = Schemas["IncidentDetail"];
/** The chat card of an incident. */
export type Card = Schemas["CardDto"];
/** One timeline line. */
export type TimelineEntry = Schemas["TimelineEntryDto"];
/** A triage report. */
export type Report = Schemas["ReportDto"];
/** A proposed or finished action. */
export type Action = Schemas["ActionDto"];
/** One page of actions. */
export type ActionPage = Schemas["ActionPage"];
/** A host and whether its agent is connected. */
export type Host = Schemas["HostDto"];
/** A change pushed over the event stream. */
export type UiEvent = Schemas["UiEvent"];
/** A host with its latest digest facts. */
export type HostDetail = Schemas["HostDetailDto"];
/** One digest fact about a host. */
export type Fact = Schemas["FactDto"];
/** A one-time enrollment invite. */
export type Invite = Schemas["InviteDto"];
/** A skill an agent reported. */
export type Skill = Schemas["SkillDto"];
/** A note. */
export type Note = Schemas["NoteDto"];
/** Creating or replacing a note. */
export type NoteInput = Schemas["NoteInput"];
/** One note search hit. */
export type NoteHit = Schemas["NoteHitDto"];
/** What a notes import did. */
export type ImportResult = Schemas["ImportResultDto"];
/** The effective configuration, secrets shown only as set or unset. */
export type Settings = Schemas["SettingsDto"];
/** An open advisory. */
export type Advisory = Schemas["AdvisoryDto"];
/** The digest as it would be sent now. */
export type DigestPreview = Schemas["DigestPreview"];

/** How the API shows a secret that has a value. */
export const SECRET_SET = "[set]";

/** A failed API call: the HTTP status and the error code from the body. */
export class ApiFailure extends Error {
  readonly status: number;
  readonly code: string;
  readonly retryAfter: number | undefined;

  constructor(status: number, code: string, message: string, retryAfter?: number) {
    super(message);
    this.name = "ApiFailure";
    this.status = status;
    this.code = code;
    this.retryAfter = retryAfter;
  }
}

interface Result<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

function errorBody(error: unknown): { error: string; message: string } {
  if (typeof error === "object" && error !== null && "error" in error && "message" in error) {
    return { error: String(error.error), message: String(error.message) };
  }
  return { error: "unknown", message: "request failed" };
}

/** The data of a successful call; throws {@link ApiFailure} otherwise. */
export async function call<T>(request: Promise<Result<T>>): Promise<T> {
  const { data, error, response } = await request;
  if (!response.ok) {
    const body = errorBody(error);
    const retry = Number(response.headers.get("Retry-After"));
    throw new ApiFailure(
      response.status,
      body.error,
      body.message,
      Number.isFinite(retry) && retry > 0 ? retry : undefined,
    );
  }
  return data as T;
}

/** `value` without its `undefined` and empty-string fields, for query parameters. */
export function defined<T extends object>(value: T): { [K in keyof T]?: Exclude<T[K], undefined> } {
  return Object.fromEntries(
    Object.entries(value).filter(([, v]) => v !== undefined && v !== ""),
  ) as { [K in keyof T]?: Exclude<T[K], undefined> };
}
