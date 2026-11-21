import { route } from "./apiMock";
import { nas } from "./fixtures";

const DEFAULT_DAYS: [string, number, string | null][] = [
  ["signals_days", 30, null],
  ["incident_timeline_days", 90, null],
  ["conversations_days", 180, null],
  ["notifications_days", 30, null],
  ["triage_cache_days", 90, null],
  ["resolved_advisories_days", 30, null],
  ["operation_journal_days", 180, "operations exist"],
  ["execution_outputs_days", 30, "execution outputs are stored apart from the audit log"],
  ["retracted_facts_days", 30, null],
];

/** The nine retention entries the hub reports; two are declared but not applied yet. */
export const RETENTION_ENTRIES = DEFAULT_DAYS.map(([key, days, once]) => ({
  key,
  days,
  default_days: days,
  applies: once ? "later" : "now",
  applies_once: once,
}));

/** The settings answer the settings page tests share; `editable` overrides part of it. */
export function settings(editable: Record<string, unknown> | null = {}) {
  return {
    hub: {
      name: "home",
      timezone: "UTC",
      bind: "127.0.0.1:7420",
      public_url: null,
      trusted_proxies: [],
    },
    llm_primary: null,
    llm_secondary: null,
    triage: {
      enabled: false,
      concurrency: 2,
      per_hour_runs: 6,
      reserved_for_incidents: 2,
      monthly_budget_usd: null,
    },
    discord: null,
    webhooks: [],
    heartbeat: { configured: false, url: "[unset]", interval: null, failing: false },
    usage: { tokens_in: 0, tokens_out: 0, cost_usd: 0 },
    retention: {
      entries: RETENTION_ENTRIES,
      backups_keep: 7,
      backups_keep_pre_migrate: 3,
    },
    editable:
      editable === null
        ? null
        : {
            revision: 3,
            values: {
              hub: { name: "home", timezone: "UTC" },
              notify: { digest: { enabled: true, time: "09:00" } },
              incidents: { confirm_runs: 2 },
              discord: { enabled: false, channel_id: "1234567890123456789", bot_token: "[set]" },
            },
            fields: {
              "hub.name": "file",
              "triage.concurrency": "env:DOMOVOI_TRIAGE__CONCURRENCY",
            },
            issues: [],
            last_apply: null,
            ...editable,
          },
  };
}

/** Registers the answers every settings page test needs besides the settings themselves. */
export function routeSettingsPage(): void {
  route("GET /api/v1/hosts", () => [
    nas,
    { ...nas, id: "host-old", name: "old", version: "0.0.0", outdated: true },
  ]);
  route("GET /api/v1/advisories", () => []);
  route("GET /api/v1/session", () => ({ csrf_token: "t", user: { id: "u", username: "admin" } }));
  route("GET /api/v1/system/update", () => ({ current: "0.1.0", latest: null, newer: false }));
  route("GET /api/v1/settings/schema", () => ({
    properties: {
      incidents: { $ref: "#/$defs/Incidents" },
      updates: { $ref: "#/$defs/Updates" },
    },
    $defs: {
      Updates: { properties: { check: { description: "Look for releases.", default: true } } },
      Incidents: {
        properties: {
          confirm_runs: {
            description: "Consecutive failing runs before an incident opens.",
            default: 2,
          },
        },
      },
    },
  }));
}
