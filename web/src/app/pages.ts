import { lazy, type ComponentType, type LazyExoticComponent } from "react";

/** A page that is downloaded when it is first needed. */
export interface LazyPage {
  Page: LazyExoticComponent<ComponentType>;
  /** Starts the download early, when a link to the page is hovered or focused. */
  preload: () => void;
}

function page(load: () => Promise<ComponentType>): LazyPage {
  // One promise per page, so a preload and the render share the same download.
  let loading: Promise<{ default: ComponentType }> | undefined;
  const once = () => (loading ??= load().then((component) => ({ default: component })));
  return {
    Page: lazy(once),
    preload: () => {
      // A failed preload is forgotten, so opening the page tries again and reports it.
      once().catch(() => (loading = undefined));
    },
  };
}

/**
 * Every page that is not part of the first download. Incidents, Login and Setup are: they
 * are what a person opens the app for, so `routes.tsx` imports them directly.
 */
export const PAGES = {
  approvals: page(() => import("../features/approvals/ApprovalsPage").then((m) => m.ApprovalsPage)),
  digest: page(() => import("../features/digest/DigestPage").then((m) => m.DigestPage)),
  firstRun: page(() => import("../features/setup/FirstRun").then((m) => m.FirstRun)),
  host: page(() => import("../features/hosts/HostDetailPage").then((m) => m.HostDetailPage)),
  hosts: page(() => import("../features/hosts/HostsPage").then((m) => m.HostsPage)),
  knowledge: page(() => import("../features/knowledge/KnowledgePage").then((m) => m.KnowledgePage)),
  fact: page(() => import("../features/knowledge/FactDetailPage").then((m) => m.FactDetailPage)),
  entity: page(() =>
    import("../features/knowledge/EntityDetailPage").then((m) => m.EntityDetailPage),
  ),
  paths: page(() => import("../features/knowledge/PathsPage").then((m) => m.PathsPage)),
  experience: page(() =>
    import("../features/experience/ExperiencePage").then((m) => m.ExperiencePage),
  ),
  audit: page(() => import("../features/audit/AuditPage").then((m) => m.AuditPage)),
  logs: page(() => import("../features/logs/LogsPage").then((m) => m.LogsPage)),
  overview: page(() => import("../features/overview/OverviewPage").then((m) => m.OverviewPage)),
  note: page(() => import("../features/notes/NoteEditorPage").then((m) => m.NoteEditorPage)),
  notes: page(() => import("../features/notes/NotesPage").then((m) => m.NotesPage)),
  operator: page(() => import("../features/operator/OperatorPage").then((m) => m.OperatorPage)),
  operations: page(() =>
    import("../features/operations/OperationsPage").then((m) => m.OperationsPage),
  ),
  operation: page(() =>
    import("../features/operations/OperationDetailPage").then((m) => m.OperationDetailPage),
  ),
  policy: page(() => import("../features/policy/PolicyPage").then((m) => m.PolicyPage)),
  policyVersion: page(() => import("../features/policy/VersionPage").then((m) => m.VersionPage)),
  policyDraft: page(() => import("../features/policy/DraftPage").then((m) => m.DraftPage)),
  policyPublish: page(() => import("../features/policy/PublishPage").then((m) => m.PublishPage)),
  policyDecision: page(() => import("../features/policy/DecisionPage").then((m) => m.DecisionPage)),
  policySimulator: page(() =>
    import("../features/policy/SimulatorPage").then((m) => m.SimulatorPage),
  ),
  rules: page(() => import("../features/rules/RulesPage").then((m) => m.RulesPage)),
  settings: page(() => import("../features/settings/SettingsPage").then((m) => m.SettingsPage)),
  skills: page(() => import("../features/skills/SkillsPage").then((m) => m.SkillsPage)),
  packageInstall: page(() =>
    import("../features/packages/InstallWizardPage").then((m) => m.InstallWizardPage),
  ),
  installJob: page(() =>
    import("../features/packages/InstallJobPage").then((m) => m.InstallJobPage),
  ),
  helperKey: page(() =>
    import("../features/capabilities/HelperKeyPage").then((m) => m.HelperKeyPage),
  ),
  capabilities: page(() =>
    import("../features/capabilities/CapabilitiesPage").then((m) => m.CapabilitiesPage),
  ),
} satisfies Record<string, LazyPage>;
