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
  chat: page(() => import("../features/chat/ChatPage").then((m) => m.ChatPage)),
  digest: page(() => import("../features/digest/DigestPage").then((m) => m.DigestPage)),
  firstRun: page(() => import("../features/setup/FirstRun").then((m) => m.FirstRun)),
  host: page(() => import("../features/hosts/HostDetailPage").then((m) => m.HostDetailPage)),
  hosts: page(() => import("../features/hosts/HostsPage").then((m) => m.HostsPage)),
  logs: page(() => import("../features/logs/LogsPage").then((m) => m.LogsPage)),
  overview: page(() => import("../features/overview/OverviewPage").then((m) => m.OverviewPage)),
  note: page(() => import("../features/notes/NoteEditorPage").then((m) => m.NoteEditorPage)),
  notes: page(() => import("../features/notes/NotesPage").then((m) => m.NotesPage)),
  rules: page(() => import("../features/rules/RulesPage").then((m) => m.RulesPage)),
  settings: page(() => import("../features/settings/SettingsPage").then((m) => m.SettingsPage)),
  skills: page(() => import("../features/skills/SkillsPage").then((m) => m.SkillsPage)),
} satisfies Record<string, LazyPage>;
