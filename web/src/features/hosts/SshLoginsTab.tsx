import { useState, type FormEvent } from "react";
import { useAckRules, useCreateAckRule, useDeleteAckRule } from "../../api/hooks/rules";
import type { AckRule, AckRuleInput, Predicate } from "../../api/types";
import {
  Button,
  Checkbox,
  ConfirmDialog,
  DocsLink,
  ErrorDetail,
  InlineMessage,
  LoadError,
  Loading,
  RelativeTime,
  useActionFeedback,
} from "../../components";
import { opLabel } from "../rules/AddRuleForm";
import styles from "./hosts.module.css";

/** The check the agent reports for a successful SSH login. */
export const SSH_CHECK = "security.ssh_login";

/** Where a login comes from, with the words in the form. `range` needs an address. */
const FROM: [string, string][] = [
  ["anywhere", "Anywhere"],
  ["lan", "Local network"],
  ["range", "This address or range"],
];

/** How long a rule lives, with the words in the form; `""` is always. */
const FOR: [string, string][] = [
  ["60", "1 hour"],
  ["1440", "1 day"],
  ["10080", "1 week"],
  ["", "Always"],
];

/** What the add form holds. */
interface Fields {
  user: string;
  from: string;
  cidr: string;
  life: string;
  note: string;
}

/** The add form starts with: anyone, from anywhere, for one hour. */
const EMPTY: Fields = {
  user: "",
  from: "anywhere",
  cidr: "",
  life: "60",
  note: "Expected SSH login",
};

/** The rule the form describes, as a request body, or the words saying what to fix. */
export function loginBody(host: string, fields: Fields): AckRuleInput | string {
  const note = fields.note.trim();
  if (note === "") {
    return "Type why the login is expected, in the note.";
  }
  const predicates: Predicate[] = [];
  if (fields.from === "lan") {
    predicates.push({ key: "network", op: "equals", value: "lan" });
  }
  if (fields.from === "range") {
    const cidr = fields.cidr.trim();
    if (cidr === "") {
      return "Type the address, or the range in CIDR form, for example 192.168.1.0/24.";
    }
    predicates.push({ key: "ip", op: "in_network", value: cidr });
  }
  return {
    host,
    check: SSH_CHECK,
    target_pattern: fields.user.trim() === "" ? null : fields.user.trim(),
    predicates,
    note,
    expires_in_minutes: fields.life === "" ? null : Number(fields.life),
  };
}

/**
 * The body of the one-hour window opened from the page: logins on `host` for one hour.
 * Local network logins are the default; internet logins are included only when asked for,
 * so one click never silences a login from outside the network.
 */
export function loginWindow(host: string, internet: boolean): AckRuleInput {
  return {
    host,
    check: SSH_CHECK,
    target_pattern: null,
    predicates: internet ? [] : [{ key: "network", op: "equals", value: "lan" }],
    note: "Login window opened from the web UI",
    expires_in_minutes: 60,
  };
}

/** Who the rule expects, in words: `alice`, or `anyone` when it has no target. */
export function userWords(rule: AckRule): string {
  return rule.target_pattern ?? "anyone";
}

/** Where logins come from, in words: `the local network`, `the range 192.168.1.0/24`, `anywhere`. */
export function fromWords(predicates: Predicate[]): string {
  for (const predicate of predicates) {
    if (predicate.key === "network" && predicate.op === "equals") {
      return predicate.value === "lan" ? "the local network" : `the ${predicate.value} network`;
    }
    if (predicate.key === "ip" && predicate.op === "in_network") {
      return predicate.value.includes("/")
        ? `the range ${predicate.value}`
        : `the address ${predicate.value}`;
    }
    if (predicate.key === "ip" && predicate.op === "equals") {
      return `the address ${predicate.value}`;
    }
  }
  if (predicates.length === 0) {
    return "anywhere";
  }
  return predicates.map((p) => `${p.key} ${opLabel(p.op)} ${p.value}`).join("; ");
}

/** A rule is an open login window when it has no target, no expiry date in the past, and
 * covers `host` (its host is `host`, or null for any host). `now` is the current ISO time. */
function isOpenWindow(
  rule: AckRule,
  host: string,
  now: string,
): rule is AckRule & {
  expires_at: string;
} {
  return (
    rule.check === SSH_CHECK &&
    (rule.host === host || rule.host === null) &&
    rule.target_pattern === null &&
    typeof rule.expires_at === "string" &&
    rule.expires_at > now
  );
}

/** The open login windows for `host`, soonest expiring first. */
function openWindows(
  rules: AckRule[],
  host: string,
  now: string,
): (AckRule & {
  expires_at: string;
})[] {
  return rules
    .filter((rule) => isOpenWindow(rule, host, now))
    .sort((a, b) => (a.expires_at < b.expires_at ? -1 : 1));
}

/** Delete one expected login, asking first. */
function RemoveButton({ rule }: { rule: AckRule }) {
  const [asking, setAsking] = useState(false);
  const remove = useActionFeedback(useDeleteAckRule(), {
    success: "Expected login removed",
    error: "remove the expected login",
    errorDisplay: "inline",
  });
  const close = () => {
    setAsking(false);
    remove.reset();
  };
  const words = `${userWords(rule)} from ${fromWords(rule.predicates)}`;
  return (
    <>
      <Button
        variant="danger"
        aria-label={`Remove the expected login ${words}`}
        onClick={() => setAsking(true)}
      >
        Remove
      </Button>
      {asking && (
        <ConfirmDialog
          title={`Remove the expected login on ${rule.host ?? "any host"}?`}
          consequence={
            <>
              <strong>{words}</strong> starts alerting again. A login that already matched stays in
              the history.
            </>
          }
          confirmLabel="Remove"
          tone="danger"
          busy={remove.isPending}
          error={remove.error}
          onConfirm={() => remove.mutate(rule.id, { onSuccess: close })}
          onCancel={close}
        >
          <p>“{rule.note}”</p>
        </ConfirmDialog>
      )}
    </>
  );
}

/**
 * Expected SSH logins on one host: the ones already marked, a form to add one, and a window
 * for logging in right now. Expected logins are recorded as incidents and never alert.
 */
export function SshLoginsTab({ host }: { host: string }) {
  const rules = useAckRules();
  const create = useActionFeedback(useCreateAckRule(), {
    success: "Expected login added",
    error: "add the expected login",
    errorDisplay: "inline",
  });
  const windowOpen = useActionFeedback(useCreateAckRule(), {
    success: `Login window opened on ${host} for 1 hour`,
    error: "open the login window",
    errorDisplay: "inline",
  });
  const [fields, setFields] = useState<Fields>(EMPTY);
  const [problem, setProblem] = useState<string>();
  const [asking, setAsking] = useState(false);
  const [internet, setInternet] = useState(false);

  const all = rules.data ?? [];
  // Rules for any host count too: they silence logins on this host as well.
  const expected = all.filter(
    (rule) => (rule.host === host || rule.host === null) && rule.check === SSH_CHECK,
  );
  const windowRule = openWindows(all, host, new Date().toISOString())[0];
  const change = (patch: Partial<Fields>) => setFields({ ...fields, ...patch });

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const body = loginBody(host, fields);
    if (typeof body === "string") {
      setProblem(body);
      return;
    }
    setProblem(undefined);
    create.mutate(body, { onSuccess: () => setFields(EMPTY) });
  };

  const openWindow = () => {
    windowOpen.mutate(loginWindow(host, internet), {
      onSuccess: () => {
        setAsking(false);
        setInternet(false);
      },
    });
  };

  return (
    <>
      <p className={styles.hint}>
        Every successful SSH login raises an incident. Logins you mark as expected are still
        recorded but never alert you.{" "}
        <DocsLink topic="expected SSH logins" docs="reference/checks.html#securityssh_login" />
      </p>

      <section aria-labelledby="ssh-window-heading">
        <h2 id="ssh-window-heading">Logging in now</h2>
        <p className={styles.hint}>
          Opens a window of one hour, in which logins on {host} are recorded and never alert. By
          default it covers logins from the local network, not from the internet.
        </p>
        <Button variant="primary" onClick={() => setAsking(true)}>
          I&rsquo;m logging in now
        </Button>
        {windowRule && (
          <p role="status">
            A login window is open on {host}, so{" "}
            {windowRule.predicates.length === 0
              ? "any login, including from the internet, is expected"
              : "logins from the local network are expected"}
            . It expires <RelativeTime at={windowRule.expires_at} />.
          </p>
        )}
        {asking && (
          <ConfirmDialog
            title={`Open a login window on ${host}?`}
            consequence={`For one hour, logins on ${host} by anyone are recorded and never alert you. Use it for the login you are about to make.`}
            confirmLabel="Open the window"
            busy={windowOpen.isPending}
            error={windowOpen.error}
            onConfirm={openWindow}
            onCancel={() => {
              setAsking(false);
              setInternet(false);
              windowOpen.reset();
            }}
          >
            <Checkbox
              label="Include logins from the internet"
              checked={internet}
              onChange={setInternet}
            />
          </ConfirmDialog>
        )}
      </section>

      <section aria-labelledby="ssh-expected-heading">
        <h2 id="ssh-expected-heading">Expected logins</h2>
        {rules.isPending && <Loading />}
        {rules.error && !rules.data && (
          <LoadError
            error={rules.error}
            context="load the expected logins"
            onRetry={rules.refetch}
          />
        )}
        {rules.data && expected.length === 0 && <p>No logins are marked as expected on {host}.</p>}
        {expected.length > 0 && (
          <ul className={styles.logins}>
            {expected.map((rule) => (
              <li key={rule.id}>
                <span>
                  <strong>{userWords(rule)}</strong> from {fromWords(rule.predicates)}
                  {rule.host === null ? " on any host" : ""},{" "}
                  {rule.expires_at ? <RelativeTime at={rule.expires_at} /> : <span>always</span>}
                </span>{" "}
                <RemoveButton rule={rule} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <form className={styles.form} onSubmit={onSubmit} aria-label="Add an expected SSH login">
        <h2>Add an expected login</h2>
        <div className={styles.grid}>
          <label className={styles.field}>
            User
            <input
              value={fields.user}
              onChange={(e) => change({ user: e.target.value })}
              placeholder="alice"
            />
          </label>
          <label className={styles.field}>
            From
            <select value={fields.from} onChange={(e) => change({ from: e.target.value })}>
              {FROM.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {fields.from === "range" && (
            <label className={styles.field}>
              Address or range
              <input
                value={fields.cidr}
                onChange={(e) => change({ cidr: e.target.value })}
                placeholder="192.168.1.20 or 192.168.1.0/24"
              />
            </label>
          )}
          <label className={styles.field}>
            For
            <select value={fields.life} onChange={(e) => change({ life: e.target.value })}>
              {FOR.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <label className={styles.field}>
          Note
          <input value={fields.note} onChange={(e) => change({ note: e.target.value })} />
        </label>
        <div className={styles.actions}>
          <Button type="submit" variant="primary" loading={create.isPending}>
            Add expected login
          </Button>
        </div>
        {problem && <InlineMessage tone="danger" title={problem} />}
        {create.error && (
          <InlineMessage tone="danger" title={create.error.title}>
            <ErrorDetail error={create.error} />
          </InlineMessage>
        )}
      </form>
    </>
  );
}
