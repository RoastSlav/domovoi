import { useState } from "react";
import type { HostCheck } from "../../api/types";
import {
  Button,
  Dialog,
  Field,
  InlineMessage,
  NumberInput,
  Select,
  Stack,
  Textarea,
} from "../../components";
import { parseDraft, type Draft, type Problems } from "./checkDraft";

function agentWords(check: HostCheck): string {
  const reported = check.reported;
  if (!reported) {
    return "The agent has not reported this check, so its own values are not known.";
  }
  return `The agent runs it ${reported.enabled ? "on" : "off"}, every ${reported.interval_seconds ?? "?"} s.`;
}

/**
 * Edits one check's overrides: whether it runs, how often and its thresholds. Apply keeps
 * the change in the page's list of unsaved changes; saving is done there. `issues` are what
 * the hub refused on the last save, each marking its field.
 */
export function CheckDialog({
  check,
  draft,
  issues,
  onApply,
  onClose,
}: {
  check: HostCheck;
  draft: Draft;
  issues: Problems;
  onApply: (draft: Draft) => void;
  onClose: () => void;
}) {
  const [fields, setFields] = useState(draft);
  const [local, setLocal] = useState<Problems>({});
  const problems = { ...issues, ...local };
  const change = (patch: Partial<Draft>) => {
    setFields({ ...fields, ...patch });
    setLocal({});
  };
  const apply = () => {
    const parsed = parseDraft(fields);
    if ("problems" in parsed) {
      setLocal(parsed.problems);
      return;
    }
    onApply(fields);
  };
  return (
    <Dialog
      title={`Edit ${check.key}`}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="ghost"
            onClick={() => setFields({ runs: "", interval: "", thresholds: "" })}
            hint="Clears every override so the agent's own values apply."
          >
            Use agent defaults
          </Button>
          <Button variant="primary" onClick={apply}>
            Apply
          </Button>
        </>
      }
    >
      <Stack gap={4}>
        <p>{agentWords(check)}</p>
        {problems.check && (
          <InlineMessage tone="danger" title="The hub refused this check">
            {problems.check}
          </InlineMessage>
        )}
        <Field
          label="Runs"
          hint="As agent default follows agent.toml. On or Off overrides it."
          error={problems.runs}
        >
          <Select
            value={fields.runs}
            onChange={(event) => change({ runs: event.target.value as Draft["runs"] })}
          >
            <option value="">As agent default</option>
            <option value="on">On</option>
            <option value="off">Off</option>
          </Select>
        </Field>
        <Field
          label="Interval in seconds"
          hint="How often the check runs. Leave it empty to use the agent's value."
          error={problems.interval}
        >
          <NumberInput
            min={1}
            value={fields.interval}
            placeholder="As agent default"
            onChange={(event) => change({ interval: event.target.value })}
          />
        </Field>
        <Field
          label="Thresholds"
          hint={
            'A JSON object merged over the agent’s, like {"warn_percent": 70}. Leave it empty to keep them.'
          }
          error={problems.thresholds}
        >
          <Textarea
            mono
            rows={3}
            spellCheck={false}
            value={fields.thresholds}
            placeholder="{}"
            onChange={(event) => change({ thresholds: event.target.value })}
          />
        </Field>
      </Stack>
    </Dialog>
  );
}
