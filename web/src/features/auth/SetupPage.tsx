import { useState, type FormEvent } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useSetup } from "../../api/hooks/session";
import { describeSetupError } from "../../design/messages";
import {
  AuthCard,
  Button,
  ErrorDetail,
  Field,
  InlineMessage,
  PasswordInput,
  TextInput,
} from "../../components";
import { MIN_PASSWORD_CHARS, PasswordRules, passwordRules } from "./PasswordRules";
import styles from "./auth.module.css";

/** First run: creates the admin account with the one-time token from the setup link. */
export function SetupPage() {
  const [params] = useSearchParams();
  const linkToken = params.get("token") ?? "";
  const [token, setToken] = useState(linkToken);
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const setup = useSetup();
  const navigate = useNavigate();

  const rules = passwordRules(password, repeat);
  const unmet = rules.find((rule) => !rule.met);
  const missing = !token.trim() || !username.trim();
  const blocked = missing
    ? "Fill in the setup token and the username first."
    : unmet
      ? unmet.problem
      : undefined;

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (blocked) {
      return;
    }
    setup.mutate(
      { token: token.trim(), username: username.trim(), password },
      { onSuccess: () => void navigate("/welcome", { replace: true }) },
    );
  };

  const failure = setup.error && !setup.isPending ? describeSetupError(setup.error) : undefined;
  return (
    <AuthCard
      title="Set up Domovoi"
      intro="Create the admin account. You only do this once."
      footer={
        <>
          Already set up? <Link to="/login">Log in</Link>
        </>
      }
    >
      <form className={styles.form} onSubmit={onSubmit} noValidate>
        {!linkToken && (
          <Field label="Setup token" hint="From the setup link in the hub's log.">
            <TextInput
              mono
              value={token}
              onChange={(event) => setToken(event.target.value)}
              autoComplete="off"
            />
          </Field>
        )}
        <Field label="Username">
          <TextInput
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            autoComplete="username"
          />
        </Field>
        <Field
          label="Password"
          hint={`Use at least ${MIN_PASSWORD_CHARS} characters. A few words in a row work well.`}
        >
          <PasswordInput
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="new-password"
          />
        </Field>
        <Field label="Repeat password">
          <PasswordInput
            subject="repeated password"
            value={repeat}
            onChange={(event) => setRepeat(event.target.value)}
            autoComplete="new-password"
          />
        </Field>
        <PasswordRules rules={rules} />
        <Button type="submit" variant="primary" loading={setup.isPending} disabledReason={blocked}>
          Create account
        </Button>
        <div className={styles.status}>
          {failure && (
            <InlineMessage tone="danger" title={failure.title}>
              <ErrorDetail error={failure} />
            </InlineMessage>
          )}
        </div>
      </form>
    </AuthCard>
  );
}
