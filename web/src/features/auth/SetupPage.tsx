import { useState, type FormEvent } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useSetup } from "../../api/hooks/session";
import { ApiFailure } from "../../api/types";
import { Button } from "../../components/Button";
import styles from "./auth.module.css";

/** The hub's shortest accepted password, in characters. */
export const MIN_PASSWORD_CHARS = 12;

function failureText(error: unknown): string {
  if (error instanceof ApiFailure) {
    if (error.status === 403) {
      return "This setup link is wrong, used or expired. Use the latest link from the hub's log.";
    }
    if (error.status === 409) {
      return "Setup is already done.";
    }
    if (error.status === 400) {
      return error.message;
    }
  }
  return "Could not create the account. Is the hub running?";
}

/** First run: creates the admin account with the one-time token from the setup link. */
export function SetupPage() {
  const [params] = useSearchParams();
  const linkToken = params.get("token") ?? "";
  const [token, setToken] = useState(linkToken);
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [problem, setProblem] = useState<string>();
  const setup = useSetup();
  const navigate = useNavigate();

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if ([...password].length < MIN_PASSWORD_CHARS) {
      setProblem(`Use at least ${MIN_PASSWORD_CHARS} characters for the password.`);
      return;
    }
    if (password !== repeat) {
      setProblem("The passwords do not match.");
      return;
    }
    setProblem(undefined);
    setup.mutate(
      { token: token.trim(), username: username.trim(), password },
      {
        onSuccess: () => void navigate("/welcome", { replace: true }),
        onError: (error) => setProblem(failureText(error)),
      },
    );
  };

  return (
    <main className={styles.page}>
      <h1>Set up Domovoi</h1>
      <p>Create the admin account. You only do this once.</p>
      <form className={styles.form} onSubmit={onSubmit} noValidate>
        {!linkToken && (
          <div className={styles.field}>
            <label htmlFor="setup-token">Setup token</label>
            <input
              id="setup-token"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              autoComplete="off"
              required
            />
            <p className={styles.hint}>From the setup link in the hub's log.</p>
          </div>
        )}
        <div className={styles.field}>
          <label htmlFor="setup-username">Username</label>
          <input
            id="setup-username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoComplete="username"
            required
          />
        </div>
        <div className={styles.field}>
          <label htmlFor="setup-password">Password</label>
          <input
            id="setup-password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="new-password"
            aria-describedby="setup-password-hint"
            required
          />
          <p id="setup-password-hint" className={styles.hint}>
            At least {MIN_PASSWORD_CHARS} characters.
          </p>
        </div>
        <div className={styles.field}>
          <label htmlFor="setup-repeat">Repeat password</label>
          <input
            id="setup-repeat"
            type="password"
            value={repeat}
            onChange={(e) => setRepeat(e.target.value)}
            autoComplete="new-password"
            required
          />
        </div>
        {problem && (
          <p className={styles.error} role="alert">
            {problem}
          </p>
        )}
        <Button type="submit" variant="primary" disabled={setup.isPending}>
          Create account
        </Button>
        <p className={styles.hint}>
          Already set up? <Link to="/login">Log in</Link>
        </p>
      </form>
    </main>
  );
}
