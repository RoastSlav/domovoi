import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { useLogin } from "../../api/hooks/session";
import { AuthCard, Button, Field, InlineError, PasswordInput, TextInput } from "../../components";
import styles from "./auth.module.css";

/** Logs in the admin. A failure says what happened in the status line under the button. */
export function LoginPage() {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const login = useLogin();
  const navigate = useNavigate();

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    login.mutate(
      { username: username.trim(), password },
      {
        onSuccess: () => void navigate("/incidents", { replace: true }),
        onError: () => setPassword(""),
      },
    );
  };

  return (
    <AuthCard title="Log in to Domovoi">
      <form className={styles.form} onSubmit={onSubmit}>
        <Field label="Username">
          <TextInput
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            autoComplete="username"
          />
        </Field>
        <Field label="Password">
          <PasswordInput
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="current-password"
          />
        </Field>
        <Button
          type="submit"
          variant="primary"
          loading={login.isPending}
          disabledReason={
            username.trim() && password ? undefined : "Enter your username and password first."
          }
        >
          Log in
        </Button>
        <div className={styles.status}>
          {login.error && !login.isPending && <InlineError error={login.error} context="log in" />}
        </div>
      </form>
    </AuthCard>
  );
}
