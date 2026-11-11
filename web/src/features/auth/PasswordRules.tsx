import { Icon } from "../../design/icons";
import styles from "./auth.module.css";

/** The hub's shortest accepted password, in characters (`MIN_PASSWORD_CHARS` in the hub). */
export const MIN_PASSWORD_CHARS = 12;

/** One thing a new password must do, and whether it does now. */
export interface PasswordRule {
  id: string;
  /** The rule as a checklist line. */
  label: string;
  met: boolean;
  /** Why the form cannot be sent while the rule is not met. */
  problem: string;
}

/** The rules a new password must meet, checked against what is typed so far. */
export function passwordRules(password: string, repeat: string): PasswordRule[] {
  return [
    {
      id: "length",
      label: `At least ${MIN_PASSWORD_CHARS} characters`,
      met: [...password].length >= MIN_PASSWORD_CHARS,
      problem: `Use at least ${MIN_PASSWORD_CHARS} characters for the password.`,
    },
    {
      id: "match",
      label: "The two passwords match",
      met: password !== "" && password === repeat,
      problem: "Type the same password in both fields.",
    },
  ];
}

/**
 * The password rules as a live checklist. A met rule has a check and the words "met"; one
 * that is not has a dot and "not met yet", so colour is never the only signal.
 */
export function PasswordRules({ rules }: { rules: PasswordRule[] }) {
  return (
    <ul className={styles.rules} aria-label="Password rules">
      {rules.map((rule) => (
        <li key={rule.id} className={styles.rule} data-tone={rule.met ? "success" : "neutral"}>
          <Icon name={rule.met ? "check-circle" : "dot"} size={14} />
          {rule.label}
          <span className="visually-hidden"> ({rule.met ? "met" : "not met yet"})</span>
        </li>
      ))}
    </ul>
  );
}
