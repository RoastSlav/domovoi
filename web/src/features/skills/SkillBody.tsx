import type { Skill } from "../../api/types";
import {
  DataTable,
  KeyValueList,
  Markdown,
  Section,
  Stack,
  StatusPill,
  Tag,
  Text,
  Tooltip,
  type Column,
} from "../../components";
import { riskMeta } from "../../design/tone";
import { SkillRuns } from "./SkillRuns";
import { paramLimits, paramType, verificationText } from "./skillText";

type Param = Skill["params"][number];

const PARAM_COLUMNS: Column<Param>[] = [
  {
    key: "name",
    label: "Name",
    cell: (param) => (
      <>
        <Text mono strong>
          {param.name}
        </Text>{" "}
        {param.required ? <Tag tone="info">Required</Tag> : <Tag>Optional</Tag>}
      </>
    ),
  },
  { key: "type", label: "Type", cell: (param) => <Text mono>{paramType(param.kind)}</Text> },
  {
    key: "limits",
    label: "Constraints",
    cell: (param) => paramLimits(param.kind) || <Text muted>None</Text>,
  },
  {
    key: "description",
    label: "Description",
    cell: (param) => param.description || <Text muted>No description</Text>,
  },
];

function Facts({ skill }: { skill: Skill }) {
  const risk = riskMeta(skill.risk);
  const hash = skill.manifest_hash;
  return (
    <KeyValueList
      items={[
        { label: "Risk", value: <StatusPill meta={risk} /> },
        { label: "Timeout", value: `${skill.timeout_secs} s` },
        {
          label: "Applies to",
          value: skill.applies_to.length > 0 ? skill.applies_to.join(", ") : "Any check",
        },
        { label: "Verification", value: verificationText(skill.verify) },
        {
          label: "Manifest hash",
          hint: "A fingerprint of the skill's manifest on the host. It changes when the skill does.",
          value: (
            <Tooltip content={hash}>
              <code>{hash.slice(0, 12)}</code>
            </Tooltip>
          ),
        },
      ]}
    />
  );
}

/** Everything about one skill: facts, parameters, README and its recent runs on `host`. */
export function SkillBody({ skill, host }: { host: string; skill: Skill }) {
  return (
    <Stack gap={4}>
      <Text muted block>
        {riskMeta(skill.risk).hint}
      </Text>
      <Facts skill={skill} />
      <Section title="Parameters">
        {skill.params.length > 0 ? (
          <DataTable
            caption={`Parameters of ${skill.name}`}
            columns={PARAM_COLUMNS}
            rows={skill.params}
            rowKey={(param) => param.name}
          />
        ) : (
          <Text muted block>
            This skill takes no parameters.
          </Text>
        )}
      </Section>
      <Section title="README">
        {skill.readme ? (
          <Markdown text={skill.readme} />
        ) : (
          <Text muted block>
            This skill has no README.
          </Text>
        )}
      </Section>
      <SkillRuns host={host} skill={skill.name} />
    </Stack>
  );
}
