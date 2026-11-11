import { Link } from "react-router";
import { useProfile, useRefreshProfile } from "../../api/hooks/profile";
import { ApiFailure, type Profile } from "../../api/types";
import {
  Button,
  Card,
  Cluster,
  EmptyState,
  InfoTip,
  InlineMessage,
  LoadError,
  Markdown,
  Progress,
  RelativeTime,
  Section,
  Skeleton,
  Stack,
  Text,
  useActionFeedback,
} from "../../components";
import { splitProfile, type ProfilePart } from "./profileText";

/** Why a tool did not answer as one sentence, ending in a full stop. */
function couldNotRead(part: ProfilePart): string {
  const reason = part.failure?.replace(/[.\s]+$/, "");
  return reason ? `Could not read ${part.title}: ${reason}.` : `Could not read ${part.title}.`;
}

/** One tool's output as a card: open and read, or a warning when the tool did not answer. */
function PartSection({ part }: { part: ProfilePart }) {
  return (
    <Section title={part.title} collapsed={part.ok}>
      {part.ok ? (
        <Card>
          <Markdown text={part.body} />
        </Card>
      ) : (
        <InlineMessage tone="warning" title={couldNotRead(part)}>
          The rest of the profile is unaffected. It tries again the next time it is collected.
        </InlineMessage>
      )}
    </Section>
  );
}

/** The profile's summary, then one collapsible card per tool. */
function ProfileBody({ profile }: { profile: Profile }) {
  const { summary, parts } = splitProfile(profile.markdown, profile.sections);
  return (
    <Stack gap={4}>
      <Section title="Summary" collapsed={false}>
        <Card>
          <Markdown text={summary} />
        </Card>
      </Section>
      {parts.map((part) => (
        <PartSection key={part.title} part={part} />
      ))}
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
