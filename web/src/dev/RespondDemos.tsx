import type { Action } from "../api/types";
import {
  ChatThread,
  DataTable,
  Grid,
  KeyValueList,
  Menu,
  Section,
  Stack,
  StatusPill,
  Text,
  Timeline,
} from "../components";
import { TIMELINE_KINDS, timelineMeta } from "../design/timeline";
import { VOCABULARIES, incidentStateMeta } from "../design/tone";
import { ActionCard } from "../features/approvals/ActionCard";
import { Demo, Group } from "./Demo";

const NOW = Date.now();

function values(vocabulary: string): string[] {
  return VOCABULARIES.find((v) => v.name === vocabulary)?.values ?? [];
}

function sample(overrides: Partial<Action>): Action {
  return {
    id: `demo-${overrides.status}-${overrides.risk}-${overrides.expires_at}`,
    incident_id: "demo",
    host_id: "demo",
    skill: "restart_container",
    risk: "write",
    args: { name: "web", compose_file: "/srv/stacks/media/docker-compose.yml" },
    status: "proposed",
    proposed_by: "system:triage",
    expires_at: new Date(NOW + 45 * 60_000).toISOString(),
    created_at: new Date(NOW - 15 * 60_000).toISOString(),
    updated_at: new Date(NOW).toISOString(),
    ...overrides,
  };
}

const TIMELINE_SUMMARIES: Record<string, string> = {
  "status.changed": "open → acked",
  verdict: "ACTION NEEDED",
};

const never = (): Promise<never> => new Promise(() => undefined);
const refuse = (): Promise<never> => Promise.reject(new TypeError("Failed to fetch"));

/** The pieces of the pages an operator responds on: actions, the timeline and the chat. */
export function RespondDemos() {
  const timeline = [...TIMELINE_KINDS, "action.succeeded", "from.a_newer_hub"].map(
    (kind, index) => ({
      at: new Date(NOW - (30 - index) * 60_000).toISOString(),
      meta: timelineMeta({
        kind,
        summary: TIMELINE_SUMMARIES[kind] ?? "The hub's own line about it",
      }),
    }),
  );
  const rows = [
    { id: "1", status: "open", verdict: "ACTION_NEEDED", title: "Disk /srv is 93% full" },
    { id: "2", status: "open", verdict: "MONITOR", title: "Load is high" },
    { id: "3", status: "resolved", verdict: null, title: "Backup was late" },
  ];
  return (
    <Group title="Respond">
      <Demo name="ActionCard: every status (approve and deny call the hub, so they fail here)">
        <Grid min="lg">
          {values("Action status").map((status) => (
            <ActionCard
              key={status}
              host="nas-01"
              action={sample({ status, reason: "exit code 1: no such container" })}
              why={
                status === "proposed" ? "The container stopped answering health checks." : undefined
              }
            />
          ))}
        </Grid>
      </Demo>
      <Demo name="ActionCard: every risk; the last one expires in under ten minutes">
        <Grid min="lg">
          {values("Skill risk").map((risk) => (
            <ActionCard key={risk} host="nas-01" action={sample({ risk })} showIncident />
          ))}
          <ActionCard
            host="nas-01"
            action={sample({ expires_at: new Date(NOW + 9 * 60_000).toISOString() })}
          />
        </Grid>
      </Demo>
      <Demo name="Timeline: every known kind, an action step and an unknown kind; long ones collapse">
        <Timeline label="Demo timeline" items={timeline} />
      </Demo>
      <Demo name="ChatThread: empty with starters">
        <ChatThread
          turns={[]}
          hint="Read-only."
          label="Ask about your servers"
          starters={["Is anything wrong on nas-01 right now?", "How full are the disks on nas-01?"]}
          onSend={refuse}
          onClear={() => undefined}
        />
      </Demo>
      <Demo name="ChatThread: a conversation with tools and a proposed action, then thinking">
        <ChatThread
          turns={[
            { role: "user", text: "Why is nas-01 slow?" },
            {
              role: "assistant",
              text: "The **media** container is restarting in a loop.\n\n- 14 restarts in the last hour\n- `exit code 137` each time",
              tools: [{ name: "read logs", host: "nas-01" }, { name: "list containers" }],
              extra: <ActionCard host="nas-01" action={sample({})} />,
            },
          ]}
          pending="Is it out of memory?"
          sending
          hint="Read-only."
          label="Ask a follow-up"
          onSend={never}
          onStop={() => undefined}
          onClear={() => undefined}
        />
      </Demo>
      <Demo name="ChatThread: send a question to see a failed answer with Retry; disabled with a reason">
        <Stack gap={5}>
          <ChatThread
            turns={[]}
            hint="Read-only."
            label="Ask (this one always fails)"
            onSend={refuse}
            onClear={() => undefined}
          />
          <ChatThread
            turns={[]}
            hint="Read-only."
            label="Ask (triage is off)"
            disabledReason="Triage is off, so there is no model to ask. Turn it on under Settings, Triage."
            onSend={refuse}
            onClear={() => undefined}
          />
        </Stack>
      </Demo>
      <Demo name="Text, KeyValueList mono, Menu with text">
        <Stack gap={3}>
          <Text block>
            Body text, <Text muted>muted</Text>, <Text mono>disk.usage /srv</Text>,{" "}
            <Text strong>strong</Text>, <Text size="sm">small</Text>, <Text size="lg">large</Text>.
          </Text>
          <KeyValueList
            mono
            items={[
              { label: "name", value: "web" },
              { label: "compose_file", value: "/srv/stacks/media/docker-compose.yml" },
            ]}
          />
          <div>
            <Menu
              text="Mute"
              label="Mute for"
              hint="No alerts or reminders for the time you pick."
              items={["For 1 hour", "For 4 hours", "For 1 day"].map((label) => ({
                label,
                onSelect: () => undefined,
              }))}
            />
          </div>
        </Stack>
      </Demo>
      <Demo name="DataTable rows: urgent (faint tint), plain, quiet; Section collapsed">
        <DataTable
          caption="Demo incidents"
          rows={rows}
          rowKey={(row) => row.id}
          rowTone={(row) => incidentStateMeta(row).tone}
          rowState={(row) =>
            row.verdict === "ACTION_NEEDED"
              ? "urgent"
              : row.status === "resolved"
                ? "quiet"
                : undefined
          }
          columns={[
            {
              key: "state",
              label: "Status",
              cell: (row) => <StatusPill meta={incidentStateMeta(row)} />,
            },
            { key: "title", label: "Incident", cell: (row) => row.title },
            {
              key: "actions",
              label: "Actions",
              hideLabel: true,
              cell: (row) => (
                <Menu
                  label={`Actions for ${row.title}`}
                  items={[{ label: "Acknowledge", icon: "check", onSelect: () => undefined }]}
                />
              ),
            },
          ]}
        />
        <Section title="Recently decided" collapsed description="Closed until asked for.">
          <Text block muted>
            Content that is only rendered, and only loaded, once the section is opened.
          </Text>
        </Section>
      </Demo>
    </Group>
  );
}
