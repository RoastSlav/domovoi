import { useHosts } from "../../api/hooks/hosts";
import {
  Field,
  LoadError,
  SegmentedControl,
  Select,
  Stack,
  Textarea,
  TextInput,
} from "../../components";
import { effectClassMeta } from "../../design/tone";
import type { Problems, RuleForm } from "./ruleModel";

const EFFECTS = ["read", "mutate", "contain", "destructive"] as const;

const CAPABILITIES: { label: string; value: RuleForm["capKind"] }[] = [
  { value: "any", label: "Any capability" },
  { value: "effect", label: "An effect class" },
  { value: "category", label: "A category" },
  { value: "contract", label: "One capability" },
];

function HostField({ hostId, onChange }: { hostId: string; onChange: (id: string) => void }) {
  const hosts = useHosts(false);
  if (hosts.isError && !hosts.data) {
    return <LoadError error={hosts.error} context="load hosts" onRetry={hosts.refetch} />;
  }
  const list = hosts.data ?? [];
  const known = hostId === "" || list.some((host) => host.id === hostId);
  return (
    <Field label="Host">
      <Select value={hostId} onChange={(e) => onChange(e.target.value)}>
        <option value="">Every host</option>
        {list.map((host) => (
          <option key={host.id} value={host.id}>
            {host.name}
          </option>
        ))}
        {!known && <option value={hostId}>{`Unknown host (${hostId.slice(0, 8)})`}</option>}
      </Select>
    </Field>
  );
}

function CapabilityValue({
  form,
  error,
  onChange,
}: {
  error: string | undefined;
  form: RuleForm;
  onChange: (patch: Partial<RuleForm>) => void;
}) {
  if (form.capKind === "any") return null;
  if (form.capKind === "effect") {
    return (
      <Field label="Effect class" error={error}>
        <Select value={form.capValue} onChange={(e) => onChange({ capValue: e.target.value })}>
          <option value="">Choose an effect class</option>
          {EFFECTS.map((effect) => (
            <option key={effect} value={effect}>
              {effectClassMeta(effect).label}
            </option>
          ))}
        </Select>
      </Field>
    );
  }
  return (
    <Field
      label={form.capKind === "category" ? "Category" : "Capability name"}
      hint="Typed as written; Check rules reports names the hub does not know."
      error={error}
    >
      <TextInput
        mono
        value={form.capValue}
        onChange={(e) => onChange({ capValue: e.target.value })}
      />
    </Field>
  );
}

/**
 * The fields that say what a rule applies to: host, capability and targets.
 * @param form The rule being edited.
 * @param problems Field problems from the last Apply.
 * @param onChange Receives the part of the form that changed.
 */
export function RuleScopeFields({
  form,
  problems,
  onChange,
}: {
  form: RuleForm;
  onChange: (patch: Partial<RuleForm>) => void;
  problems: Problems;
}) {
  return (
    <Stack gap={4}>
      <HostField hostId={form.hostId} onChange={(hostId) => onChange({ hostId })} />
      <Field label="Capability">
        <Select
          value={form.capKind}
          onChange={(e) =>
            onChange({ capKind: e.target.value as RuleForm["capKind"], capValue: "" })
          }
        >
          {CAPABILITIES.map((cap) => (
            <option key={cap.value} value={cap.value}>
              {cap.label}
            </option>
          ))}
        </Select>
      </Field>
      <CapabilityValue form={form} error={problems.capValue} onChange={onChange} />
      <SegmentedControl
        label="Targets"
        value={form.targetsKind}
        onChange={(targetsKind) => onChange({ targetsKind })}
        options={[
          { value: "any", label: "Any target" },
          { value: "exact", label: "Named targets" },
        ]}
      />
      {form.targetsKind === "exact" && (
        <Field
          label="One target name per line"
          hint="Names match exactly, including case."
          error={problems.targetNames}
        >
          <Textarea
            mono
            rows={4}
            value={form.targetNames}
            onChange={(e) => onChange({ targetNames: e.target.value })}
          />
        </Field>
      )}
    </Stack>
  );
}
