import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { useLogin } from "../../api/hooks/session";
import { ApiFailure } from "../../api/types";
import { Button } from "../../components/Button";
import styles from "./auth.module.css";

/** What a failed login says. It never tells which of username and password was wrong. */
export function loginFailureText(error: unknown): string {
  if (error instanceof ApiFailure) {
    if (error.status === 401) {
      return "Wrong username or password.";
    }
    if (error.status === 429) {
      const minutes = Math.max(1, Math.ceil((error.retryAfter ?? 60) / 60));
      return `Too many attempts. Try again in ${minutes} minute${minutes === 1 ? "" : "s"}.`;
    }
  }
  return "Could not log in. Is the hub running?";
}

/** Logs in the admin. */
export function LoginPage() {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [problem, setProblem] = useState<string>();
  const login = useLogin();
  const navigate = useNavigate();

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    setProblem(undefined);
    login.mutate(
      { username: username.trim(), password },
      {
        onSuccess: () => void navigate("/incidents", { replace: true }),
        onError: (error) => {
          setPassword("");
          setProblem(loginFailureText(error));
        },
      },
    );
  };

  return (
    <main className={styles.page}>
      <h1>Log in to Domovoi</h1>
      <form className={styles.form} onSubmit={onSubmit}>
        <div className={styles.field}>
          <label htmlFor="login-username">Username</label>
          <input
            id="login-username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoComplete="username"
            required
          />
        </div>
        <div className={styles.field}>
          <label htmlFor="login-password">Password</label>
          <input
            id="login-password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
            required
          />
        </div>
        {problem && (
          <p className={styles.error} role="alert">
            {problem}
          </p>
        )}
        <Button type="submit" variant="primary" disabled={login.isPending}>
          Log in
        </Button>
      </form>
    </main>
  );
}
