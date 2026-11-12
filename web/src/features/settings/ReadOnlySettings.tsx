import { Fragment, type ReactNode } from "react";
import { SECRET_SET, type Settings } from "../../api/types";
import { InlineMessage, Section, Stack, Tag, Text, type KeyValue } from "../../components";
import styles from "./layout.module.css";
import { RowHeading, SettingsCard, ValueRow } from "./SettingRow";
import { AboutSection } from "./views/AboutSection";
import { Advisories } from "./views/Advisories";
import { BudgetMeter, budgetOf, usageRows } from "./views/Usage";

type Llm = NonNullable<Settings["llm_primary"]>;

/**
 * A field the API marks secret. Only whether it is set is ever shown; any other value is
 * withheld rather than printed.
 */
export function SecretValue({ value }: { value: string }) {
  if (value === SECRET_SET) {
    return <Tag tone="success">set</Tag>;
  }
  if (value === "[unset]") {
    return <Tag>not set</Tag>;
  }
  return <Tag>hidden</Tag>;
}

const yesNo = (value: boolean): string => (value ? "yes" : "no");

function llmRows(llm: Llm): KeyValue[] {
  return [
    { label: "Provider", value: llm.kind },
    { label: "Model", value: llm.model ?? "default" },
    { label: "Endpoint", value: llm.base_url ?? "default" },
    { label: "API key", value: <SecretValue value={llm.api_key} /> },
  ];
}

/** Values as rows of one card, in the look of the editable settings. */
function ValueRows({ rows }: { rows: KeyValue[] }) {
  return (
    <SettingsCard>
      {rows.map((row) => (
        <ValueRow key={row.label} label={row.label}>
          {row.value}
        </ValueRow>
      ))}
    </SettingsCard>
  );
}

function Block({
  title,
  rows,
  children,
}: {
  title: string;
  rows?: KeyValue[];
  children?: ReactNode;
}) {
  return (
    <Section title={title}>
      <Stack gap={3}>
        {rows && rows.length > 0 && <ValueRows rows={rows} />}
        {children}
      </Stack>
    </Section>
  );
}

function Webhooks({ hooks }: { hooks: Settings["webhooks"] }) {
  if (hooks.length === 0) {
    return <Text muted>No webhooks are configured.</Text>;
  }
  return (
    <SettingsCard>
      {hooks.map((hook) => (
        <Fragment key={hook.name}>
          <RowHeading>{`/hooks/${hook.name}`}</RowHeading>
          <ValueRow label="Kind">{hook.kind}</ValueRow>
          <ValueRow label="Authentication">{hook.auth}</ValueRow>
          <ValueRow label="Secret">
            <SecretValue value={hook.secret} />
          </ValueRow>
          {hook.default_host && <ValueRow label="Host">{hook.default_host}</ValueRow>}
        </Fragment>
      ))}
    </SettingsCard>
  );
}

/**
 * The settings of a hub whose configuration is managed by file: everything it runs with,
 * read-only. Secrets show only as set or not set.
 */
export function ReadOnlySettings({ settings: s }: { settings: Settings }) {
  const heartbeat = s.heartbeat.failing ? (
    <Tag tone="danger">Failing</Tag>
  ) : (
    <Tag tone="success">OK</Tag>
  );
  return (
    <div className={styles.readOnly}>
      <Stack gap={6}>
        <InlineMessage tone="info" title="Settings are managed by file">
          <p>Read-only. Change settings in hub.toml and restart the hub.</p>
        </InlineMessage>
        <Block
          title="Hub"
          rows={[
            { label: "Name", value: s.hub.name },
            { label: "Time zone", value: s.hub.timezone },
            { label: "Web listener", value: s.hub.bind },
            { label: "Public URL", value: s.hub.public_url ?? "not set" },
            { label: "Trusted proxies", value: s.hub.trusted_proxies.join(", ") || "none" },
          ]}
        />
        <Block
          title="Triage"
          rows={[
            { label: "Enabled", value: yesNo(s.triage.enabled) },
            { label: "Runs at once", value: String(s.triage.concurrency) },
            { label: "Runs per hour", value: String(s.triage.per_hour_runs) },
            ...usageRows(s),
            ...(budgetOf(s) === undefined
              ? []
              : [{ label: "Budget used", value: <BudgetMeter settings={s} /> }]),
          ]}
        />
        <Block title="Primary model" rows={s.llm_primary ? llmRows(s.llm_primary) : []}>
          {!s.llm_primary && <Text muted>Not configured.</Text>}
        </Block>
        {s.llm_secondary && <Block title="Fallback model" rows={llmRows(s.llm_secondary)} />}
        <Block
          title="Discord"
          rows={
            s.discord
              ? [
                  { label: "Enabled", value: yesNo(s.discord.enabled) },
                  { label: "Channel", value: s.discord.channel_id },
                  {
                    label: "Allowed users",
                    value: s.discord.allowed_user_ids.join(", ") || "none",
                  },
                  { label: "Bot token", value: <SecretValue value={s.discord.bot_token} /> },
                  { label: "Webhook URL", value: <SecretValue value={s.discord.webhook_url} /> },
                ]
              : []
          }
        >
          {!s.discord && <Text muted>Not configured.</Text>}
        </Block>
        <Block title="Webhooks">
          <Webhooks hooks={s.webhooks} />
        </Block>
        <Block
          title="Heartbeat"
          rows={[
            { label: "Configured", value: yesNo(s.heartbeat.configured) },
            { label: "URL", value: <SecretValue value={s.heartbeat.url} /> },
            { label: "Interval", value: s.heartbeat.interval ?? "not set" },
            { label: "Status", value: heartbeat },
          ]}
        />
        <Block title="Advisories">
          <Advisories />
        </Block>
        <Block title="About and updates">
          <AboutSection />
        </Block>
      </Stack>
    </div>
  );
}
