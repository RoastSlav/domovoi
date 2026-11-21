import type {
  PolicyDraft,
  PolicyOverview,
  PolicyPreview,
  PolicySimulation,
  PolicyVersionDetail,
  ShadowDetail,
  ShadowPage,
} from "../api/types";
import decisionError from "./fixtures/policy/decision-error.json";
import decisionException from "./fixtures/policy/decision-exception.json";
import decisionLimits from "./fixtures/policy/decision-limits.json";
import decisionLocked from "./fixtures/policy/decision-locked.json";
import decisionUnavailable from "./fixtures/policy/decision-unavailable.json";
import decisionUnevaluable from "./fixtures/policy/decision-unevaluable.json";
import draft4 from "./fixtures/policy/draft-4.json";
import overviewActive from "./fixtures/policy/overview-active.json";
import overviewBerlin from "./fixtures/policy/overview-berlin.json";
import overviewFresh from "./fixtures/policy/overview-fresh.json";
import previewBlocking from "./fixtures/policy/preview-blocking.json";
import previewClean from "./fixtures/policy/preview-clean.json";
import previewInvalid from "./fixtures/policy/preview-invalid.json";
import shadowPageJson from "./fixtures/policy/shadow-page.json";
import simulateBoth from "./fixtures/policy/simulate-both.json";
import version3 from "./fixtures/policy/version-3.json";

/** An overview with a published version, a draft and a recent sweep. */
export const OVERVIEW_ACTIVE = overviewActive as unknown as PolicyOverview;
/** An overview before any policy exists. */
export const OVERVIEW_FRESH = overviewFresh as unknown as PolicyOverview;
/** An overview in a non-UTC zone, with a stale draft and a recorder error. */
export const OVERVIEW_BERLIN = overviewBerlin as unknown as PolicyOverview;
/** Published version 3 with its rules and one acknowledgement. */
export const VERSION_3 = version3 as unknown as PolicyVersionDetail;
/** Draft revision 4, based on version 3. */
export const DRAFT_4 = draft4 as unknown as PolicyDraft;
/** A review with nothing to acknowledge. */
export const PREVIEW_CLEAN = previewClean as unknown as PolicyPreview;
/** A review with findings that need acknowledging and a locked change. */
export const PREVIEW_BLOCKING = previewBlocking as unknown as PolicyPreview;
/** A review of rules that do not validate. */
export const PREVIEW_INVALID = previewInvalid as unknown as PolicyPreview;
/** A simulation of draft revision 4 over four recorded steps. */
export const SIMULATE_BOTH = simulateBoth as unknown as PolicySimulation;
/** One page of recorded decisions with a next cursor. */
export const SHADOW_PAGE_ONE = shadowPageJson as unknown as ShadowPage;
/** A decision a locked Deny decided. */
export const DECISION_LOCKED = decisionLocked as unknown as ShadowDetail;
/** A decision a specific Allow won over a broader Deny. */
export const DECISION_EXCEPTION = decisionException as unknown as ShadowDetail;
/** A decision narrowed by hard limits. */
export const DECISION_LIMITS = decisionLimits as unknown as ShadowDetail;
/** A decision that could not be reached because a safety input is unknown. */
export const DECISION_UNAVAILABLE = decisionUnavailable as unknown as ShadowDetail;
/** A step that could not be evaluated. */
export const DECISION_UNEVALUABLE = decisionUnevaluable as unknown as ShadowDetail;
/** A decision whose enforced side holds an error code. */
export const DECISION_ERROR = decisionError as unknown as ShadowDetail;

/** Every fixture by file name, for tests that walk them all. */
export const POLICY_FIXTURES = {
  "overview-active": OVERVIEW_ACTIVE,
  "overview-fresh": OVERVIEW_FRESH,
  "overview-berlin": OVERVIEW_BERLIN,
  "version-3": VERSION_3,
  "draft-4": DRAFT_4,
  "preview-clean": PREVIEW_CLEAN,
  "preview-blocking": PREVIEW_BLOCKING,
  "preview-invalid": PREVIEW_INVALID,
  "simulate-both": SIMULATE_BOTH,
  "shadow-page": SHADOW_PAGE_ONE,
  "decision-locked": DECISION_LOCKED,
  "decision-exception": DECISION_EXCEPTION,
  "decision-limits": DECISION_LIMITS,
  "decision-unavailable": DECISION_UNAVAILABLE,
  "decision-unevaluable": DECISION_UNEVALUABLE,
  "decision-error": DECISION_ERROR,
} as const;
