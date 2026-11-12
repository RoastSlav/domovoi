import { Link } from "react-router";
import { useProfile, useRefreshProfile } from "../../api/hooks/profile";
import { ApiFailure, type Profile } from "../../api/types";
import {
  Button,
  Card,
  Cluster,
  EmptyState,
  InfoTip,
  LoadError,
  Markdown,
  Progress,
  RelativeTime,
  Skeleton,
  Stack,
  Text,
  useActionFeedback,
} from "../../components";
import { ProfileCards, FactTiles } from "./ProfileCards";
import { foldOverview } from "./profileOverview";
import { hasFacts, readFacts } from "./profileFacts";
import { splitProfile } from "./profileText";

/** The summary as tiles, then the cards; text the page cannot read shows as written. */
function ProfileBody({ profile }: { profile: Profile }) {
  const split = splitProfile(profile.markdown, profile.sections);
  const { summary } = split;
  const { parts, tiles } = foldOverview(split.parts);
  const facts = readFacts(summary);
  return (
    <Stack gap={4}>
      {hasFacts(facts) || tiles.length > 0 ? (
        <FactTiles facts={facts} overview={tiles} />
      ) : (
        summary && (
          <Card>
            <Markdown text={summary} />
          </Card>
        )
      )}
      <ProfileCards facts={facts} parts={parts} />
    </Stack>
  );
}

/** What the tab shows while the hub has no profile: that one is being built, or why not. */
function NoProfile({ connected }: { connected: boolean }) {
  if (!connected) {
    return (
      <EmptyState icon="server" title="No profile yet">
        <p>
          A profile is collected from the agent. Connect an agent to profile this machine, then it
          appears here.
        </p>
      </EmptyState>
    );
  }
  return (
    <Progress label="Building the baseline. This takes a few minutes after the agent connects." />
  );
}

/**
 * What this machine looks like while it behaves normally, as the hub collected it. The hub
 * checks every minute and collects when there is none, when the agent version or machine
 * facts changed, or a week has passed; the button collects one now. Polls only until a
 * profile exists.
 */
export function ProfileTab({ host, connected }: { host: string; connected: boolean }) {
  const profile = useProfile(host);
  const refresh = useActionFeedback(useRefreshProfile(host), {
    success: "Profile collected",
    error: "collect the profile",
  });

  if (profile.isPending) {
    return <Skeleton shape="rows" count={4} />;
  }
  if (profile.error || !profile.data) {
    const missing = profile.error instanceof ApiFailure && profile.error.status === 404;
    return missing ? (
      <NoProfile connected={connected} />
    ) : (
      <LoadError error={profile.error} context="load the profile" onRetry={profile.refetch} />
    );
  }
  const seen = profile.data;
  return (
    <Stack gap={4}>
      <Cluster justify="between">
        <Cluster gap={2}>
          <Text>
            Collected <RelativeTime at={seen.collected_at} />
          </Text>
          <InfoTip title="Profile">
            What this machine looks like when things are normal: its services, containers, ports and
            hardware. Triage compares an incident against it. The hub checks every minute and
            collects a new one when there is none, when the machine changed or a week has passed.
          </InfoTip>
        </Cluster>
        <Cluster gap={3}>
          {seen.note && <Link to={`/notes/${seen.note}`}>Profile note</Link>}
          {connected && (
            <Button icon="refresh" onClick={() => refresh.mutate(host)} loading={refresh.isPending}>
              Refresh
            </Button>
          )}
        </Cluster>
      </Cluster>
      <ProfileBody profile={seen} />
    </Stack>
  );
}
