import type { components } from "./schema";

type Schemas = components["schemas"];

/** The logged-in user and the session's CSRF token. */
export type Session = Schemas["SessionDto"];
export type Preferences = Schemas["PreferencesDto"];
export type PresentationPrefs = Schemas["PresentationDto"];
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
/** What the model still remembers of the follow-up conversation. */
export type FollowUps = Schemas["FollowUpsDto"];
/** One follow-up question and its answer. */
export type Turn = Schemas["TurnDto"];
/** A triage report. */
export type Report = Schemas["ReportDto"];
/** How the latest triage run went. */
export type TriageState = Schemas["TriageStateDto"];
/** A proposed or finished action. */
export type Action = Schemas["ActionDto"];
/** One page of actions. */
export type ActionPage = Schemas["ActionPage"];
/** The counts the navigation shows: incidents, approvals and offline hosts. */
export type Overview = Schemas["OverviewDto"];
/** A host and whether its agent is connected. */
export type Host = Schemas["HostDto"];
/** A host with its latest digest facts. */
export type HostDetail = Schemas["HostDetailDto"];
/** One digest fact about a host. */
export type Fact = Schemas["FactDto"];
/** A one-time enrollment invite. */
export type Invite = Schemas["InviteDto"];
/** A skill an agent reported. */
export type Skill = Schemas["SkillDto"];
/** One capability row of a host's inventory. */
export type Capability = Schemas["CapabilityDto"];
/** One host's capability inventory. */
export type Capabilities = Schemas["CapabilitiesDto"];
/** What one host reported as its unattended limits. */
export type HostCeiling = Schemas["CeilingDto"];
/** One capability's bound in a host's limits. */
export type CeilingEntry = Schemas["CeilingEntryDto"];
/** The package store's status and the owner key list. */
export type PackagesStatus = Schemas["PackagesStatusDto"];
/** An imported package. */
export type PackageSummary = Schemas["PackageDto"];
/** An imported package with its signature file and the hosts holding it. */
export type PackageDetail = Schemas["PackageDetailDto"];
/** The plan the hub computed for an install or removal. */
export type InstallPlan = Schemas["InstallPlanDto"];
/** One host's part of a plan. */
export type InstallPlanTarget = Schemas["PlanTargetDto"];
/** An install job. */
export type InstallJob = Schemas["InstallJobDto"];
/** One host's attempt within a job. */
export type InstallTarget = Schemas["InstallTargetDto"];
/** One page of install jobs. */
export type JobPage = Schemas["JobPageDto"];
/** What a host reported it has installed. */
export type HostPackages = Schemas["HostPackagesDto"];
/** One package a host holds. */
export type HostPackage = Schemas["HostPackageDto"];
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
/** The last release check, with commands that update the hub and its agents. */
export type Update = Schemas["UpdateDto"];
/** The settings as the settings page edits them, with each field's source. */
export type Editable = Schemas["EditableSettings"];
/** A saved settings change. */
export type Saved = Schemas["SavedDto"];
/** The result of a connection test. */
export type TestResult = Schemas["TestResultDto"];
/** A host's checks: what its agent reports and what the hub overrides. */
export type HostChecks = Schemas["HostChecksDto"];
export type HostDocker = Schemas["HostDockerDto"];
/** One check on a host. */
export type HostCheck = Schemas["HostCheckDto"];
/** One check's settings. */
export type CheckSettings = Schemas["CheckSettingsDto"];
/** What the hub knows a machine looks like while it behaves normally. */
export type Profile = Schemas["ProfileDto"];
/** One tool output inside a profile; a failed one is a visible gap. */
export type ProfileSection = Schemas["ProfileSectionDto"];
/** One problem with submitted settings, with its path. */
export type Issue = Schemas["IssueDto"];
/** A known condition: incidents that match it are kept in the history and never notify. */
export type AckRule = Schemas["AckRuleDto"];
/** One condition on a signal's evidence. */
export type Predicate = Schemas["PredicateDto"];
/** How a condition compares an evidence value with its value. */
export type PredicateOp = Schemas["PredicateOpDto"];
/** A new rule. */
export type AckRuleInput = Schemas["AckRuleInput"];
/** Dismissing an incident as a known condition: the reason, and how long the check stays quiet. */
export type DismissInput = Schemas["DismissRequest"];
/** One line from the hub's own log. */
export type LogLine = Schemas["LogLineDto"];

/** How the API shows a secret that has a value. */
export const SECRET_SET = "[set]";

/** A failed API call: the HTTP status, the error code and, for invalid settings, each issue. */
export class ApiFailure extends Error {
  readonly status: number;
  readonly code: string;
  readonly retryAfter: number | undefined;
  readonly issues: Issue[];

  constructor(
    status: number,
    code: string,
    message: string,
    retryAfter?: number,
    issues: Issue[] = [],
  ) {
    super(message);
    this.name = "ApiFailure";
    this.status = status;
    this.code = code;
    this.retryAfter = retryAfter;
    this.issues = issues;
  }
}

interface Result<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

function issuesOf(error: object): Issue[] {
  if (!("issues" in error) || !Array.isArray(error.issues)) {
    return [];
  }
  return error.issues.filter(
    (issue): issue is Issue =>
      typeof issue === "object" && issue !== null && "path" in issue && "message" in issue,
  );
}

function errorBody(error: unknown): { error: string; message: string; issues: Issue[] } {
  if (typeof error === "object" && error !== null && "error" in error && "message" in error) {
    return { error: String(error.error), message: String(error.message), issues: issuesOf(error) };
  }
  return { error: "unknown", message: "request failed", issues: [] };
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
      body.issues,
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

/** What the hub knows about a host's helper: its key, the attestation and the verified set. */
export type HelperView = Schemas["HelperDto"];
/** The verified effective set of a host's helper. */
export type HelperVerified = Schemas["VerifiedDto"];

/** One operation in a list. */
export type OperationSummary = Schemas["OperationSummaryDto"];
/** One page of operations. */
export type OperationPage = Schemas["OperationPage"];
/** An operation with its trace and executions. */
export type OperationDetail = Schemas["OperationDetailDto"];
/** How an operation unfolded, as the journal rows replay rebuilds past states from. */
export type OperationHistory = Schemas["OperationHistoryDto"];
/** One journal row of an operation, reduced to ids, a time, a kind and a state. */
export type HistoryEvent = Schemas["HistoryEventDto"];
/** A run a person approves or denies. */
export type Execution = Schemas["ExecutionDto"];
/** One step with its stored intent and bounded output. */
export type StepDetail = Schemas["StepDetailDto"];
/** An operation after a control. */
export type OperationControl = Schemas["OperationControlDto"];
/** One limit to raise when resuming. */
export type BudgetExtension = Schemas["BudgetExtensionDto"];

/** The summary of a finished operation: each field with its source, and the history. */
export type ResolutionSummary = Schemas["ResolutionSummaryDto"];
/** One field of a summary. */
export type SummaryField = Schemas["SummaryFieldDto"];
/** What an edit of a narrative field returns. */
export type EditSummaryResponse = Schemas["EditSummaryResponse"];
/** A finished operation, as every list of past operations shows it. */
export type PastOperation = Schemas["PastOperationDto"];
/** Past operations that resemble an operation or an incident. */
export type SimilarOperations = Schemas["SimilarOperationsDto"];
/** The owner's search over past operations. */
export type ExperienceSearch = Schemas["ExperienceSearchDto"];

/** One fact in the knowledge store, with its source, status and freshness. */
export type KnowledgeFact = Schemas["KnowledgeFactDto"];
/** One page of knowledge facts. */
export type KnowledgeFactPage = Schemas["KnowledgeFactPageDto"];
/** A fact the owner asserts. */
export type KnowledgeFactInput = Schemas["KnowledgeFactInput"];
/** An owner's action on a fact: the version they read and, for safety-critical ones, the password. */
export type KnowledgeFactAction = Schemas["KnowledgeFactAction"];
/** A management path of a host: the fact, its state and who confirmed it. */
export type ManagementPath = Schemas["ManagementPathDto"];
/** A host's management paths and whether the helper's coverage can be checked. */
export type ManagementPaths = Schemas["ManagementPathsDto"];
/** A relation between two subjects. */
export type KnowledgeRelation = Schemas["RelationDto"];
/** One page of relations. */
export type KnowledgeRelationPage = Schemas["RelationPageDto"];
/** A relation the owner asserts. */
export type KnowledgeRelationInput = Schemas["RelationInput"];
/** A thing the owner names so facts can be about it. */
export type KnowledgeEntity = Schemas["EntityDto"];

/** A conversation with the Operator, without its messages. */
export type ConversationSummary = Schemas["ConversationSummaryDto"];
/** A conversation with its messages and the turn in progress. */
export type ConversationDetail = Schemas["ConversationDetailDto"];
/** One page of conversations. */
export type ConversationPage = Schemas["ConversationPage"];
/** One question or answer of a conversation. */
export type ConversationMessage = Schemas["MessageDto"];
/** The committed question and its pending answer. */
export type SentMessage = Schemas["SendResponse"];
/** What a conversation is about, as the hub stores it. */
export type ConversationScope = Schemas["ScopeDto"];
/** What a new conversation is about. */
export type ScopeChoice = Schemas["ScopeRequest"];

/** The policy at a glance: active version, draft and the shadow recorder. */
export type PolicyOverview = Schemas["PolicyOverviewDto"];
/** One published policy version, without its rules. */
export type PolicyVersionSummary = Schemas["VersionSummaryDto"];
/** A published version with its rules and acknowledgements. */
export type PolicyVersionDetail = Schemas["VersionDetailDto"];
/** One page of published versions. */
export type PolicyVersionPage = Schemas["VersionPageDto"];
/** The stored draft rule set. */
export type PolicyDraft = Schemas["DraftDetailDto"];
/** The draft in the overview, without its rules. */
export type PolicyDraftSummary = Schemas["DraftSummaryDto"];
/** The answer to saving a draft. */
export type PolicyDraftSaved = Schemas["DraftSavedDto"];
/** One policy rule as the hub stores it. */
export type PolicyRule = Schemas["RuleDto"];
/** The review of a rule set: problems, findings, locked changes and a simulation. */
export type PolicyPreview = Schemas["PreviewDto"];
/** One conflict or note in a publication report. */
export type PolicyFinding = Schemas["FindingDto"];
/** One problem found in a rule set. */
export type PolicyValidation = Schemas["ValidationDto"];
/** A conflict somebody acknowledged when publishing. */
export type PolicyAck = Schemas["AckDto"];
/** The findings of a publication report. */
export type PolicyReport = Schemas["PolicyReportDto"];
/** A change to a locked rule. */
export type LockChange = Schemas["LockChangeDto"];
/** What a simulation found. */
export type PolicySimulation = Schemas["SimulationDto"];
/** One recorded step with its re-run decisions. */
export type SimulationItem = Schemas["SimulationItemDto"];
/** A re-run decision next to the recorded one. */
export type Replayed = Schemas["ReplayedDto"];
/** How a simulation changed the counts. */
export type SimulationSummary = Schemas["SimulationSummaryDto"];
/** One page of recorded decisions. */
export type ShadowPage = Schemas["ShadowPageDto"];
/** One recorded decision in a list. */
export type ShadowRow = Schemas["ShadowSummaryRowDto"];
/** One recorded decision with everything needed to replay it. */
export type ShadowDetail = Schemas["ShadowDetailDto"];

/** One page of audit entries. */
export type AuditPage = Schemas["AuditPageDto"];
/** One audit entry. */
export type AuditRow = Schemas["AuditRowDto"];
/** The result of checking the audit log. */
export type AuditVerification = Schemas["AuditVerificationDto"];
/** The retention periods in force. */
export type RetentionSettings = Schemas["RetentionSettings"];
/** Whether unattended action is paused, and the rules whose breaker is open. */
export type Autonomy = Schemas["AutonomyDto"];
/** One rule whose circuit breaker is open. */
export type SuspendedRule = Schemas["SuspendedRuleDto"];
