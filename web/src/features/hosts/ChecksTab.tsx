import { useState, type FormEvent } from "react";
import { useHostChecks, useSaveHostChecks } from "../../api/hooks/settings";
import { ApiFailure, type HostChecks, type Issue } from "../../api/types";
import {
  EmptyState,
  ErrorDetail,
  InlineMessage,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  SubmitBar,
  useActionFeedback,
} from "../../components";
import { CheckDialog } from "./CheckDialog";
import { CheckTable } from "./CheckTable";
import { draftOf, overridesOf, problemsFor, sameDraft, type Draft } from "./checkDraft";
import { useFailingChecks } from "./checkState";
import { DockerPanel } from "./DockerPanel";

function NoChecks({ connected }: { connected: boolean }) {
  return (
    <EmptyState icon="info" title="No checks yet">
      <p>
        {connected
          ? "The agent reported no checks. Look at its agent.toml: a check that is not listed there does not run."
          : "The agent is offline, so its checks are not known yet. They appear here once it connects."}
      </p>
    </EmptyState>
  );
}

/** A host's checks as a table, edited one at a time in a dialog and saved together. */
function Checks({ host, data }: { data: HostChecks; host: string }) {
  const failing = useFailingChecks(host);
  const save = useActionFeedback(useSaveHostChecks(host), {
    success: (saved) =>
      saved.delivered
        ? "Checks saved and sent to the agent"
        : {
            title: "Checks saved",
            detail: "The agent is offline. They apply when it reconnects.",
            tone: "info",
          },
    error: "save the checks",
    errorDisplay: "inline",
  });
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [editing, setEditing] = useState<string>();
  const [issues, setIssues] = useState<Issue[]>([]);

  const draftFor = (key: string): Draft => {
    const found = data.checks.find((check) => check.key === key);
    return drafts[key] ?? (found ? draftOf(found) : { runs: "", interval: "", thresholds: "" });
  };
  const changes = data.checks.filter((check) => {
    const staged = drafts[check.key];
    return staged !== undefined && !sameDraft(staged, draftOf(check));
  }).length;
  const editedCheck = data.checks.find((check) => check.key === editing);

  const apply = (key: string, draft: Draft) => {
    setDrafts({ ...drafts, [key]: draft });
    setIssues(issues.filter((issue) => issue.path !== key && !issue.path.startsWith(`${key}.`)));
    setEditing(undefined);
  };
  const discard = () => {
    setDrafts({});
    setIssues([]);
    save.reset();
  };
  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const all = Object.fromEntries(data.checks.map((check) => [check.key, draftFor(check.key)]));
    save.mutate(overridesOf(all), {
      onSuccess: () => {
        setDrafts({});
        setIssues([]);
      },
      onError: (failure) => {
        const refused = failure instanceof ApiFailure ? failure.issues : [];
        setIssues(refused);
        // Open the first check the hub refused, with its fields marked.
        setEditing(
          data.checks.find((c) => Object.keys(problemsFor(c.key, refused)).length > 0)?.key,
        );
      },
    });
  };

  return (
    <>
      <form onSubmit={onSubmit} noValidate>
        <Stack gap={4}>
          {!data.connected && (
            <InlineMessage tone="info" title="The agent is offline">
              Changes are saved and sent when it reconnects.
            </InlineMessage>
          )}
          {save.error && (
            <InlineMessage tone="danger" title={save.error.title}>
              <ErrorDetail error={save.error} />
            </InlineMessage>
          )}
          <CheckTable
            checks={data.checks}
            connected={data.connected}
            drafts={drafts}
            failing={failing.keys}
            onEdit={setEditing}
          />
          <SubmitBar
            changes={changes}
            onDiscard={discard}
            saving={save.isPending}
            saveLabel="Save checks"
          />
        </Stack>
      </form>
      {editedCheck && editing && (
        <CheckDialog
          key={editing}
          check={editedCheck}
          draft={draftFor(editing)}
          issues={problemsFor(editing, issues)}
          onApply={(draft) => apply(editing, draft)}
          onClose={() => setEditing(undefined)}
        />
      )}
    </>
  );
}

/**
 * A host's checks: what its agent.toml sets, and the hub's overrides on top. Saving sends
 * them to the agent at once, or at its next connection when it is offline. The container
 * runtime setting is below them.
 */
export function ChecksTab({ host }: { host: string }) {
  const checks = useHostChecks(host);
  return (
    <Stack gap={5}>
      <Section
        title="Checks"
        description="What this host's agent runs. Edit a check to override its schedule or thresholds."
      >
        <QueryBoundary
          query={checks}
          context="load the checks"
          loading={<Skeleton shape="rows" count={4} />}
          isEmpty={(data) => data.checks.length === 0}
          empty={<NoChecks connected={checks.data?.connected ?? false} />}
        >
          {(data) => <Checks host={host} data={data} />}
        </QueryBoundary>
      </Section>
      <DockerPanel host={host} />
    </Stack>
  );
}
