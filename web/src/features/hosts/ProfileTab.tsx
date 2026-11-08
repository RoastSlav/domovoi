import { Link } from "react-router";
import { useProfile, useRefreshProfile } from "../../api/hooks/profile";
import { ApiFailure, type ProfileSection } from "../../api/types";
import { Button } from "../../components/Button";
import { Markdown } from "../../components/Markdown";
import { RelativeTime } from "../../components/RelativeTime";
import styles from "./hosts.module.css";

/** What a failed API call says to a person; server-side faults get no useful words. */
function failureText(error: unknown): string {
  if (error instanceof ApiFailure && error.status < 500) {
    return error.message;
  }
  return "Something went wrong. Try again.";
}

/** The sections whose tool did not answer, as one line of gaps. */
function Gaps({ sections }: { sections: ProfileSection[] }) {
  const gaps = sections
    .filter((section) => !section.ok)
    .map((section) => section.title)
    .join(", ");
  if (gaps === "") {
    return null;
  }
  return <p className={styles.hint}>Not available: {gaps}</p>;
}

/**
 * What this machine looks like while it behaves normally, as the hub collected it.
 * The hub checks every minute and collects when there is none, when the agent version or
 * machine facts changed, or a week has passed; the button collects one now.
 */
export function ProfileTab({ host, connected }: { host: string; connected: boolean }) {
  const profile = useProfile(host);
  const refresh = useRefreshProfile(host);

  if (profile.isPending) {
    return <p>Loading…</p>;
  }
  if (profile.error || !profile.data) {
    const missing = profile.error instanceof ApiFailure && profile.error.status === 404;
    if (!missing) {
      return (
        <p className={styles.hint} role="alert">
          {failureText(profile.error)}
        </p>
      );
    }
    return (
      <p className={styles.hint}>
        {connected
          ? "Collecting the first profile… the hub collects one within a minute."
          : "Connect an agent to profile this machine."}
      </p>
    );
  }
  const seen = profile.data;
  return (
    <>
      <p className={styles.hint}>
        What this machine looks like when things are normal. The hub checks every minute and
        collects when there is none, when the machine changed or a week passed.
      </p>
      <p>
        Collected <RelativeTime at={seen.collected_at} />
        {seen.note && (
          <>
            {" · "}
            <Link to={`/notes/${seen.note}`}>Profile note</Link>
          </>
        )}
      </p>
      <Gaps sections={seen.sections} />
      <Markdown text={seen.markdown} />
      {connected && (
        <Button onClick={() => refresh.mutate(host)} disabled={refresh.isPending}>
          Refresh
        </Button>
      )}
      {refresh.error && (
        <p className={styles.hint} role="alert">
          {failureText(refresh.error)}
        </p>
      )}
    </>
  );
}
