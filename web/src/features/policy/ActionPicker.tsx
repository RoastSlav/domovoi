import { useState } from "react";
import {
  Disclosure,
  Field,
  InlineMessage,
  Select,
  Stack,
  StatusPill,
  Text,
  TextInput,
} from "../../components";
import { effectClassMeta } from "../../design/tone";
import { categoryTitle } from "./choiceHelpers";
import { usePolicyChoices } from "./policyChoices";
import type { RuleForm } from "./ruleModel";

const EFFECTS = ["read", "mutate", "contain", "destructive"] as const;

const EFFECT_SENTENCES: Record<(typeof EFFECTS)[number], string> = {
  read: "everything that only reads",
  mutate: "everything that changes something and can be undone",
  contain: "everything that limits damage, such as stopping or isolating",
  destructive: "everything that removes something for good",
};

type Kind = RuleForm["capKind"];
const NO_VALUE = "none:";

function encode(kind: Kind, value: string): string {
  if (kind === "any") return "any";
  return value === "" ? NO_VALUE : `${kind}:${value}`;
}

function decode(encoded: string): Pick<RuleForm, "capKind" | "capValue"> {
  const at = encoded.indexOf(":");
  if (encoded === "any" || at < 0) return { capKind: "any", capValue: "" };
  return { capKind: encoded.slice(0, at) as Kind, capValue: encoded.slice(at + 1) };
}

function Selected({ form, describe }: { describe: string; form: RuleForm }) {
  if (form.capKind === "effect" && form.capValue !== "") {
    return (
      <Text muted block>
        {effectClassMeta(form.capValue).hint}
      </Text>
    );
  }
  if (form.capKind === "contract" || form.capKind === "category") {
    return (
      <Text muted block>
        {describe}
        {describe === "" ? "" : " "}
        <Text mono muted>
          {form.capValue}
        </Text>
      </Text>
    );
  }
  return null;
}

/**
 * Chooses what a rule applies to: any action, a kind of action, a group of actions or one
 * action. The choices come from the capability inventory of the rule's host. A value the
 * inventory does not list stays and is labelled. Typing a name is a small fallback.
 * @param form The rule being edited.
 * @param error The hint for the capability, from the last Apply.
 * @param onChange Receives the part of the form that changed.
 */
export function ActionPicker({
  form,
  error,
  onChange,
}: {
  error?: string | undefined;
  form: RuleForm;
  onChange: (patch: Partial<RuleForm>) => void;
}) {
  const choices = usePolicyChoices(form.hostId);
  const [typedKind, setTypedKind] = useState<"contract" | "category">(
    form.capKind === "category" ? "category" : "contract",
  );
  const current = encode(form.capKind, form.capValue);
  const action = choices.actions.find((a) => a.contract === form.capValue);
  const listed =
    form.capKind === "any" ||
    form.capKind === "effect" ||
    current === NO_VALUE ||
    (form.capKind === "category" && choices.categories.some((c) => c.category === form.capValue)) ||
    (form.capKind === "contract" && action !== undefined);
  const unreadable = choices.failed || (!choices.loading && choices.actions.length === 0);
  const group = choices.categories.find((c) => c.category === form.capValue);
  const describe =
    form.capKind === "contract"
      ? (action?.describe ?? "")
      : group
        ? `${group.count} ${group.count === 1 ? "action" : "actions"}.`
        : "";
  const missing = choices.loading ? form.capValue : `Not listed on this host: ${form.capValue}`;

  return (
    <Stack gap={2}>
      <Field label="Applies to" hint="Pick from what the host offers." error={error}>
        <Select value={current} onChange={(e) => onChange(decode(e.target.value))}>
          {current === NO_VALUE && <option value={NO_VALUE}>Choose what it applies to</option>}
          {!listed && <option value={current}>{missing}</option>}
          <option value="any">Anything (any action)</option>
          <optgroup label="A kind of action">
            {EFFECTS.map((effect) => (
              <option key={effect} value={`effect:${effect}`}>
                {`${effectClassMeta(effect).label}: ${EFFECT_SENTENCES[effect]}`}
              </option>
            ))}
          </optgroup>
          {choices.categories.length > 0 && (
            <optgroup label="A group of actions">
              {choices.categories.map(({ category, count }) => (
                <option key={category} value={`category:${category}`}>
                  {`${categoryTitle(category)} (${count} ${count === 1 ? "action" : "actions"})`}
                </option>
              ))}
            </optgroup>
          )}
          {choices.actions.length > 0 && (
            <optgroup label="One action">
              {choices.actions.map((a) => (
                <option key={a.contract} value={`contract:${a.contract}`}>
                  {`${a.title} (${a.contract})`}
                </option>
              ))}
            </optgroup>
          )}
        </Select>
      </Field>
      {action && <StatusPill meta={effectClassMeta(action.effect)} size="sm" />}
      <Selected form={form} describe={describe} />
      {unreadable && (
        <InlineMessage tone="warning" title="This host's actions could not be loaded.">
          You can still name a group or an action by typing it below.
        </InlineMessage>
      )}
      <Disclosure label="Type a name that is not listed" reveal={unreadable}>
        <Stack gap={2}>
          <Field label="The name is">
            <Select
              value={typedKind}
              onChange={(e) =>
                setTypedKind(e.target.value === "category" ? "category" : "contract")
              }
            >
              <option value="contract">One action, such as container.restart</option>
              <option value="category">A group, such as container.lifecycle</option>
            </Select>
          </Field>
          <Field
            label="Name"
            hint="Typed as written; Check rules reports names the hub does not know."
          >
            <TextInput
              mono
              value={form.capKind === typedKind ? form.capValue : ""}
              onChange={(e) => onChange({ capKind: typedKind, capValue: e.target.value })}
            />
          </Field>
        </Stack>
      </Disclosure>
    </Stack>
  );
}
