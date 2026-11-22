/**
 * The history of one operation as the hub's history endpoint returns it. These mirror the
 * generated schema (`OperationHistoryDto`, `HistoryEventDto`); the aliases in `api/types.ts`
 * replace them once the generated types land.
 */

/** One journal row of the operation, reduced to ids, a time, a kind and a state. */
export interface HistoryEvent {
  /** RFC 3339 UTC time the row was recorded. */
  at: string;
  /** The stored journal kind; a reader must tolerate kinds it does not know. */
  kind: string;
  seq: number;
  /** The step the row is about, when it names one. */
  step: string | null;
  /** The new execution state or operation status the row records. */
  state: string | null;
}

/** How an operation unfolded, oldest row first. */
export interface OperationHistory {
  as_of_seq: number;
  complete: boolean;
  events: HistoryEvent[];
  /** `"pruned"`, `"capped"`, `"both"` or null when complete. */
  incomplete_reason: string | null;
}
