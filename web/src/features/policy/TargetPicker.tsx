import { useState } from "react";
import {
  Button,
  Checkbox,
  Cluster,
  Field,
  IconButton,
  InlineMessage,
  SegmentedControl,
  Stack,
  Tag,
  Text,
  TextInput,
} from "../../components";
import { usePolicyChoices } from "./policyChoices";
import type { RuleForm } from "./ruleModel";
import { splitTargets } from "./ruleModel";

/** With more known names than this, the list gets a box to filter it. */
const FILTER_FROM = 8;
const CONTROL = /[\u0000-\u001f\u007f]/; // eslint-disable-line no-control-regex

function nameProblem(name: string, taken: readonly string[]): string | undefined {
  if (name === "" || name.length > 128 || CONTROL.test(name)) {
    return "A target name is 1 to 128 characters with no control characters.";
  }
  return taken.includes(name) ? "That name is already in the list." : undefined;
}

/**
 * Chooses the targets a rule applies to: any target, or only the ones ticked. Known names
 * come from the host's facts; a name that is not listed can be added by typing it.
 * @param form The rule being edited.
 * @param error The hint for the names, from the last Apply.
 * @param onChange Receives the part of the form that changed.
 */
export function TargetPicker({
  form,
  error,
  onChange,
}: {
  error?: string | undefined;
  form: RuleForm;
  onChange: (patch: Partial<RuleForm>) => void;
}) {
  const choices = usePolicyChoices(form.hostId);
  const [filter, setFilter] = useState("");
  const [typed, setTyped] = useState("");
  const [typedProblem, setTypedProblem] = useState<string | undefined>();
  const selected = splitTargets(form.targetNames);
  const hint = choices.actions.find((a) => a.contract === form.capValue)?.targetKind;
  const known = choices.targets(form.capKind === "contract" ? hint : undefined);
  const all = [...known, ...selected.filter((name) => !known.includes(name))];
  const wanted = filter.trim().toLowerCase();
  const shown = wanted ? all.filter((name) => name.toLowerCase().includes(wanted)) : all;

  const set = (names: string[]) => onChange({ targetNames: names.join("\n") });
  const toggle = (name: string, on: boolean) =>
    set(on ? [...selected, name] : selected.filter((n) => n !== name));
  const add = () => {
    const name = typed.trim();
    const problem = nameProblem(name, selected);
    setTypedProblem(problem);
    if (problem) return;
    set([...selected, name]);
    setTyped("");
  };

  return (
    <Stack gap={3}>
      <SegmentedControl
        label="Targets"
        value={form.targetsKind}
        onChange={(targetsKind) => onChange({ targetsKind })}
        options={[
          { value: "any", label: "Any target" },
          { value: "exact", label: "Only these" },
        ]}
      />
      {form.targetsKind === "exact" && (
        <>
          {selected.length > 0 && (
            <Cluster gap={2}>
              {selected.map((name) => (
                <Tag key={name}>
                  {name}
                  <IconButton
                    icon="close"
                    size="sm"
                    label={`Remove ${name}`}
                    onClick={() => toggle(name, false)}
                  />
                </Tag>
              ))}
            </Cluster>
          )}
          {all.length === 0 && !choices.loading && (
            <InlineMessage tone="info">
              {choices.failed
                ? "The host's known names could not be loaded. Add the names you mean below."
                : "No known targets for this host yet. Add the names you mean below."}
            </InlineMessage>
          )}
          {all.length > FILTER_FROM && (
            <Field label="Filter the list">
              <TextInput value={filter} onChange={(e) => setFilter(e.target.value)} />
            </Field>
          )}
          {shown.length > 0 && (
            <Stack gap={1}>
              {shown.map((name) => (
                <Checkbox
                  key={name}
                  label={name}
                  hint={known.includes(name) ? undefined : "Not listed on this host"}
                  checked={selected.includes(name)}
                  onChange={(on) => toggle(name, on)}
                />
              ))}
            </Stack>
          )}
          <Field
            label="Add another name"
            hint="For a name that is not listed. Names match exactly, including case."
            error={typedProblem ?? error}
          >
            <Cluster gap={2}>
              <TextInput
                mono
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    add();
                  }
                }}
              />
              <Button onClick={add}>Add</Button>
            </Cluster>
          </Field>
          {selected.length === 0 && (
            <Text muted block>
              Nothing chosen yet.
            </Text>
          )}
        </>
      )}
    </Stack>
  );
}
