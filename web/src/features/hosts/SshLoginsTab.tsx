import { useState, type FormEvent } from "react";
import { useAckRules, useCreateAckRule, useDeleteAckRule } from "../../api/hooks/rules";
import { ApiFailure, type AckRule, type AckRuleInput, type Predicate } from "../../api/types";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { PageIntro } from "../../components/PageIntro";
import { RelativeTime } from "../../components/RelativeTime";
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

/** The body of the one-hour window opened from the page: any login on the host is expected. */
export function loginWindow(host: string): AckRuleInput {
  return {
    host,
    check: SSH_CHECK,
    target_pattern: null,
    predicates: [],
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
      return `the range ${predicate.value}`;
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

/** Delete one expected login. */
function RemoveButton({ rule }: { rule: AckRule }) {
  const remove = useDeleteAckRule();
  return (
    <Button
      variant="danger"
      aria-label={`Remove the expected login ${userWords(rule)} from ${fromWords(rule.predicates)}`}
      disabled={remove.isPending}
      onClick={() => remove.mutate(rule.id)}
    >
      Remove
    </Button>
  );
}

/**
 * Expected SSH logins on one host: the ones already marked, a form to add one, and a window
 * for logging in right now. Expected logins are recorded as incidents and never alert.
 */
export function SshLoginsTab({ host }: { host: string }) {
  const rules = useAckRules();
  const create = useCreateAckRule();
  const [fields, setFields] = useState<Fields>(EMPTY);
  const [problem, setProblem] = useState<string>();
  const [windowUntil, setWindowUntil] = useState<string>();

  const expected = (rules.data ?? []).filter(
    (rule) => rule.host === host && rule.check === SSH_CHECK,
  );
  const change = (patch: Partial<Fields>) => setFields({ ...fields, ...patch });

  const show = (error: unknown) =>
    setProblem(error instanceof ApiFailure ? error.message : "Could not add the expected login.");

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const body = loginBody(host, fields);
    if (typeof body === "string") {
      setProblem(body);
      return;
    }
    setProblem(undefined);
    create.mutate(body, {
      onSuccess: () => setFields(EMPTY),
      onError: show,
    });
  };

  const openWindow = () => {
    setProblem(undefined);
    create.mutate(loginWindow(host), {
      onSuccess: (rule) => setWindowUntil(rule.expires_at ?? undefined),
      onError: show,
    });
  };

  return (
    <>
      <PageIntro topic="expected SSH logins" docs="reference/checks.html#securityssh_login">
        Every successful SSH login raises an incident. Logins you mark as expected are still
        recorded but never alert you.
      </PageIntro>

      <section aria-labelledby="ssh-window-heading">
        <h2 id="ssh-window-heading">Logging in now</h2>
        <p className={styles.hint}>
          Opens a window of one hour, in which any login by anyone on {host} is expected.
        </p>
        <Button variant="primary" disabled={create.isPending} onClick={openWindow}>
          I&rsquo;m logging in now
        </Button>
        {windowUntil && (
          <p role="status">
            A login window is open on {host}, so any login is expected. It expires{" "}
            <RelativeTime at={windowUntil} />.
          </p>
        )}
      </section>

      <section aria-labelledby="ssh-expected-heading">
        <h2 id="ssh-expected-heading">Expected logins</h2>
        {rules.isPending && <p>Loading…</p>}
        {rules.error && <EmptyState title="Could not load the known conditions" />}
        {rules.data && expected.length === 0 && <p>No logins are marked as expected on {host}.</p>}
        {expected.length > 0 && (
          <ul className={styles.logins}>
            {expected.map((rule) => (
              <li key={rule.id}>
                <span>
                  <strong>{userWords(rule)}</strong> from {fromWords(rule.predicates)},{" "}
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
          <Button type="submit" variant="primary" disabled={create.isPending}>
            Add expected login
          </Button>
        </div>
        {problem && (
          <p className={styles.error} role="alert">
            {problem}
          </p>
        )}
      </form>
    </>
  );
}
