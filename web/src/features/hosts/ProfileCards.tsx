import type { ReactNode } from "react";
import {
  Card,
  Cluster,
  CopyBlock,
  Grid,
  InlineMessage,
  KeyValueList,
  Stack,
  StatTile,
  Tag,
  Text,
} from "../../components";
import { keyValues, type ProfileFacts } from "./profileFacts";
import type { ProfilePart } from "./profileText";
import type { OverviewTile } from "./profileOverview";
import styles from "./profile.module.css";
import { TileGrid } from "./TileGrid";

/** Why a tool did not answer as one sentence, ending in a full stop. */
function couldNotRead(part: ProfilePart): string {
  const reason = part.failure?.replace(/[.\s]+$/, "");
  return reason ? `Could not read ${part.title}: ${reason}.` : `Could not read ${part.title}.`;
}

/** A card with a clear heading over its content. */
function ProfileCard({ title, children }: { children: ReactNode; title: string }) {
  return (
    <Card label={title}>
      <Stack gap={2}>
        <Text strong>{title}</Text>
        {children}
      </Stack>
    </Card>
  );
}

/** The tiles of the summary's facts, then of the overview's numbers. */
export function FactTiles({ facts, overview }: { facts: ProfileFacts; overview: OverviewTile[] }) {
  const { containers } = facts;
  const tiles = [
    facts.os && <StatTile key="os" label="System" value={facts.os} hint={facts.arch} />,
    facts.kernel && <StatTile key="kernel" label="Kernel" value={facts.kernel} />,
    facts.cpus && <StatTile key="cpus" label="CPUs" value={facts.cpus} hint="logical" />,
    facts.memory && !overview.some((tile) => tile.label === "Memory") && (
      <StatTile key="memory" label="Installed memory" value={facts.memory} />
    ),
    containers && (
      <StatTile
        key="containers"
        label="Containers"
        value={containers.total}
        hint={containers.total > 0 ? `${containers.running} running` : "none on this host"}
      />
    ),
    facts.agent && <StatTile key="agent" label="Agent" value={facts.agent} />,
    ...overview.map((tile) => (
      <StatTile
        key={tile.label}
        label={tile.label}
        value={tile.value}
        hint={tile.hint}
        tone={tile.tone}
      />
    )),
  ].filter(Boolean);
  return <TileGrid tiles={tiles} />;
}

/** One tool's output: key and value pairs when it reads like that, else a scrolling mono block. */
function PartCard({ part }: { part: ProfilePart }) {
  const pairs = keyValues(part.text);
  return (
    <ProfileCard title={part.title}>
      {pairs ? (
        <KeyValueList mono items={pairs} />
      ) : (
        <div className={styles.scroll}>
          <CopyBlock label={`${part.title} output`} text={part.text} />
        </div>
      )}
    </ProfileCard>
  );
}

function Names({ names }: { names: readonly string[] }) {
  return (
    <Cluster gap={1}>
      {names.map((name) => (
        <Tag key={name}>{name}</Tag>
      ))}
    </Cluster>
  );
}

/**
 * The cards of a profile, in a grid of two columns on a wide screen: what changed, one card
 * per tool that answered, then the skills and checks. A tool that failed is a warning above
 * them, not an empty card.
 */
export function ProfileCards({ facts, parts }: { facts: ProfileFacts; parts: ProfilePart[] }) {
  const answered = parts.filter((part) => part.ok && part.text !== "");
  const failed = parts.filter((part) => !part.ok);
  const lists = [
    { label: "Read skills", names: facts.skills },
    { label: "Checks", names: facts.checks },
  ].filter((list) => list.names.length > 0);
  return (
    <Stack gap={4}>
      {failed.map((part) => (
        <InlineMessage key={part.title} tone="warning" title={couldNotRead(part)}>
          The rest of the profile is unaffected. It tries again the next time it is collected.
        </InlineMessage>
      ))}
      <Grid min="lg">
        {facts.changes.length > 0 && (
          <ProfileCard title="Recent changes">
            <ul className={styles.list}>
              {facts.changes.map((change) => (
                <li key={change}>{change}</li>
              ))}
            </ul>
          </ProfileCard>
        )}
        {answered.map((part) => (
          <PartCard key={part.title} part={part} />
        ))}
        {lists.length > 0 && (
          <ProfileCard title="Skills and checks">
            <KeyValueList
              items={lists.map((list) => ({
                label: list.label,
                value: <Names names={list.names} />,
              }))}
            />
          </ProfileCard>
        )}
      </Grid>
    </Stack>
  );
}
