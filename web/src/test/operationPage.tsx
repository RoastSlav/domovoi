import { ProfileProvider } from "./visibility";
import type { ProfileId } from "../design/visibility";
import { RightSlotHost, RightSlotProvider } from "../app/rightSlot";
import type { OperationDetail, StepDetail } from "../api/types";
import { OperationDetailPage } from "../features/operations/OperationDetailPage";
import { route } from "./apiMock";
import { nas } from "./fixtures";
import { renderAt } from "./render";
import { stubSlotEnvironment } from "./rightSlot";

/** What the hub would send for one step of `detail`, with a short output. */
export function stepDetail(detail: OperationDetail, stepId: string): StepDetail {
  const step = detail.trace.steps.find((candidate) => candidate.id === stepId);
  return {
    id: stepId,
    operation_id: detail.operation.id,
    ordinal: 1,
    kind: step?.kind ?? "note",
    phase: step?.phase ?? "diagnose",
    parent: null,
    cycle: step?.cycle ?? 0,
    outcome: step?.outcome ?? "succeeded",
    started_at: step?.started_at ?? null,
    ended_at: step?.ended_at ?? null,
    summary: step?.summary ?? null,
    reason: null,
    output: "plex is down",
    intent: { kind: step?.kind ?? "note", text: "Which container?" },
    created_at: "2026-01-01T00:00:00Z",
  } as unknown as StepDetail;
}

/**
 * Renders the operation page in the shell's frame (provider and slot host), answering the
 * hub's routes from `detail`. `search` is the page's query string, for a deep link.
 */
export function showOperation(
  detail: OperationDetail,
  {
    search = "",
    width = 1300,
    get,
    summary,
    similar,
    profile = "detailed",
    pages = {},
  }: {
    search?: string;
    width?: number;
    get?: () => unknown;
    /** The answer to the operation's resolution summary; by default "not ready". */
    summary?: () => unknown;
    /** The answer to its similar past operations; by default none. */
    similar?: () => unknown;
    profile?: ProfileId;
    pages?: Record<string, ProfileId>;
  } = {},
) {
  stubSlotEnvironment({ width, wide: true });
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/operations/{id}", get ?? (() => detail));
  route(
    "GET /api/v1/operations/{id}/summary",
    summary ??
      (() => ({
        operation_id: detail.operation.id,
        as_of_seq: detail.as_of_seq,
        state: "not_ready",
        ended_at: null,
        fields: [],
        history: [],
        history_truncated: false,
        label: "historical evidence, not authorization",
      })),
  );
  route(
    "GET /api/v1/operations/{id}/similar",
    similar ??
      (() => ({
        items: [],
        as_of_seq: detail.as_of_seq,
        label: "historical evidence, not authorization",
      })),
  );
  route("GET /api/v1/operations/{id}/steps/{step}", (init) =>
    stepDetail(detail, init.params?.path?.step ?? ""),
  );
  return renderAt(
    `/operations/${detail.operation.id}${search}`,
    "/operations/:id",
    <ProfileProvider profile={profile} pages={pages}>
      <RightSlotProvider>
        <main id="main" tabIndex={-1}>
          <OperationDetailPage />
        </main>
        <RightSlotHost />
      </RightSlotProvider>
    </ProfileProvider>,
  );
}
