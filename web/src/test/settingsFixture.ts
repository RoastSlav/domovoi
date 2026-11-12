import { route } from "./apiMock";
import { nas } from "./fixtures";

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
    properties: { incidents: { $ref: "#/$defs/Incidents" } },
    $defs: {
      Incidents: {
        properties: {
          confirm_runs: { description: "Consecutive failing runs before an incident opens." },
        },
      },
    },
  }));
}
