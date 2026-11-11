import { Link } from "react-router";
import { useProfile, useRefreshProfile } from "../../api/hooks/profile";
import { ApiFailure, type ProfileSection } from "../../api/types";
import {
  Button,
  LoadError,
  Loading,
  Markdown,
  RelativeTime,
  useActionFeedback,
} from "../../components";
import styles from "./hosts.module.css";

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
  const refresh = useActionFeedback(useRefreshProfile(host), {
    success: "Profile collected",
    error: "collect the profile",
  });

  if (profile.isPending) {
    return <Loading />;
  }
  if (profile.error || !profile.data) {
    const missing = profile.error instanceof ApiFailure && profile.error.status === 404;
    if (!missing) {
      return (
        <LoadError error={profile.error} context="load the profile" onRetry={profile.refetch} />
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
        <Button icon="refresh" onClick={() => refresh.mutate(host)} loading={refresh.isPending}>
          Refresh
        </Button>
      )}
    </>
  );
}
