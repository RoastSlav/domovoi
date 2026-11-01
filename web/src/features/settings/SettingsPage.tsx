import type { ReactNode } from "react";
import { useHostNames } from "../../api/hooks/hosts";
import { useAdvisories, useSettings } from "../../api/hooks/system";
import { SECRET_SET, type Settings } from "../../api/types";
import { EmptyState } from "../../components/EmptyState";
import { RelativeTime } from "../../components/RelativeTime";
import { EditableSettings } from "./EditableSettings";
import styles from "./settings.module.css";

type Llm = NonNullable<Settings["llm_primary"]>;

/**
 * A field the API marks secret. Only whether it is set is ever shown; any other value is
 * withheld rather than printed.
 */
export function SecretValue({ value }: { value: string }) {
  if (value === SECRET_SET) {
    return <span className={styles.set}>set</span>;
  }
  if (value === "[unset]") {
    return <span className={styles.unset}>not set</span>;
  }
  return <span className={styles.unset}>hidden</span>;
}

function Rows({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className={styles.rows}>
      {rows.map(([label, value]) => (
        <div key={label} className={styles.row}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  const id = `settings-${title.toLowerCase().replace(/\W+/g, "-")}`;
  return (
    <section className={styles.section} aria-labelledby={id}>
      <h2 id={id}>{title}</h2>
      {children}
    </section>
  );
}

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

function llmRows(llm: Llm): [string, ReactNode][] {
  return [
    ["Provider", llm.kind],
    ["Model", llm.model ?? "default"],
    ["Endpoint", llm.base_url ?? "default"],
    ["API key", <SecretValue key="key" value={llm.api_key} />],
  ];
}

function Usage({ settings }: { settings: Settings }) {
  const { usage, triage } = settings;
  const budget = triage.monthly_budget_usd;
  const spent = `$${usage.cost_usd.toFixed(2)}`;
  return (
    <>
      <p>
        {spent}
        {budget != null
          ? ` of $${budget.toFixed(2)} this month`
          : " this month, no budget set"} · {usage.tokens_in.toLocaleString()} tokens in,{" "}
        {usage.tokens_out.toLocaleString()} out
      </p>
      {budget != null && budget > 0 && (
        <meter
          className={styles.meter}
          min={0}
          max={budget}
          high={budget * 0.8}
          value={Math.min(usage.cost_usd, budget)}
          aria-label="Budget used this month"
        >
          {spent}
        </meter>
      )}
    </>
  );
}

function Advisories() {
  const advisories = useAdvisories();
  const hosts = useHostNames();
  if (advisories.isPending) {
    return <p>Loading…</p>;
  }
  if (advisories.error) {
    return <p>Could not load advisories.</p>;
  }
  if (advisories.data.length === 0) {
    return <p>None open.</p>;
  }
  return (
    <ul className={styles.advisories}>
      {advisories.data.map((advisory) => (
        <li key={advisory.id}>
          <strong>{advisory.title}</strong>
          {advisory.host_id && ` · ${hosts.get(advisory.host_id) ?? "unknown host"}`} ·{" "}
          <RelativeTime at={advisory.created_at} />
          <p>{advisory.detail}</p>
        </li>
      ))}
    </ul>
  );
}

/**
 * The settings: editable sections when the hub manages them (each field shows where its
 * value comes from), then status that is always read-only. Secrets show only as set or
 * not set.
 */
export function SettingsPage() {
  const settings = useSettings();
  if (settings.isPending) {
    return <p>Loading…</p>;
  }
  if (settings.error) {
    return <EmptyState title="Could not load settings" />;
  }
  const s = settings.data;
  if (s.editable) {
    return (
      <>
        <h1>Settings</h1>
        <p className={styles.note}>
          Changes apply right away: the hub restarts in a few seconds. Fields set in hub.toml or the
          environment are read-only here.
        </p>
        <EditableSettings editable={s.editable} />
        <h2 className={styles.statusHeading}>Status</h2>
        <Section title="Model usage">
          <Usage settings={s} />
        </Section>
        <Section title="Heartbeat status">
          <p>{s.heartbeat.failing ? <strong className={styles.failing}>Failing</strong> : "OK"}</p>
        </Section>
        <Section title="Advisories">
          <Advisories />
        </Section>
      </>
    );
  }
  return (
    <>
      <h1>Settings</h1>
      <p className={styles.note}>Read-only. Change settings in hub.toml and restart the hub.</p>
      <Section title="Hub">
        <Rows
          rows={[
            ["Name", s.hub.name],
            ["Time zone", s.hub.timezone],
            ["Web listener", s.hub.bind],
            ["Public URL", s.hub.public_url ?? "not set"],
            ["Trusted proxies", s.hub.trusted_proxies.join(", ") || "none"],
          ]}
        />
      </Section>
      <Section title="Triage">
        <Rows
          rows={[
            ["Enabled", yesNo(s.triage.enabled)],
            ["Runs at once", String(s.triage.concurrency)],
            ["Runs per hour", String(s.triage.per_hour_runs)],
          ]}
        />
        <Usage settings={s} />
      </Section>
      <Section title="Primary model">
        {s.llm_primary ? <Rows rows={llmRows(s.llm_primary)} /> : <p>Not configured.</p>}
      </Section>
      {s.llm_secondary && (
        <Section title="Fallback model">
          <Rows rows={llmRows(s.llm_secondary)} />
        </Section>
      )}
      <Section title="Discord">
        {s.discord ? (
          <Rows
            rows={[
              ["Enabled", yesNo(s.discord.enabled)],
              ["Channel", s.discord.channel_id],
              ["Allowed users", s.discord.allowed_user_ids.join(", ") || "none"],
              ["Bot token", <SecretValue key="bot" value={s.discord.bot_token} />],
              ["Webhook URL", <SecretValue key="hook" value={s.discord.webhook_url} />],
            ]}
          />
        ) : (
          <p>Not configured.</p>
        )}
      </Section>
      <Section title="Webhooks">
        {s.webhooks.length === 0 ? (
          <p>None configured.</p>
        ) : (
          <ul className={styles.webhooks}>
            {s.webhooks.map((hook) => (
              <li key={hook.name}>
                <code>/hooks/{hook.name}</code> · {hook.kind} · {hook.auth} · secret{" "}
                <SecretValue value={hook.secret} />
                {hook.default_host && ` · host ${hook.default_host}`}
              </li>
            ))}
          </ul>
        )}
      </Section>
      <Section title="Heartbeat">
        <Rows
          rows={[
            ["Configured", yesNo(s.heartbeat.configured)],
            ["URL", <SecretValue key="url" value={s.heartbeat.url} />],
            ["Interval", s.heartbeat.interval ?? "not set"],
            [
              "Status",
              s.heartbeat.failing ? <strong className={styles.failing}>Failing</strong> : "OK",
            ],
          ]}
        />
      </Section>
      <Section title="Advisories">
        <Advisories />
      </Section>
    </>
  );
}
