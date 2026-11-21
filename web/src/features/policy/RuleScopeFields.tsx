import { useHosts } from "../../api/hooks/hosts";
import { Field, LoadError, Select, Stack } from "../../components";
import { ActionPicker } from "./ActionPicker";
import { TargetPicker } from "./TargetPicker";
import type { Problems, RuleForm } from "./ruleModel";

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
      <ActionPicker form={form} error={problems.capValue} onChange={onChange} />
      <TargetPicker form={form} error={problems.targetNames} onChange={onChange} />
    </Stack>
  );
}
