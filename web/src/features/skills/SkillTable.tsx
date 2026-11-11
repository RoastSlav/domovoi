import type { ReactNode } from "react";
import type { Skill } from "../../api/types";
import {
  Button,
  Cluster,
  DataTable,
  RelativeTime,
  Stack,
  StatusPill,
  Switch,
  Tag,
  Text,
  type Column,
} from "../../components";
import { riskMeta } from "../../design/tone";
import { RISK_EXPLANATION, sortByRisk } from "./skillText";

/** What the table needs from its owner: what to do with a skill, and what to add to its rows. */
export interface SkillTableProps {
  /** The host the skills belong to; it names the table and the switches' explanation. */
  host: string;
  onOpen: (skill: Skill) => void;
  onToggle: (skill: Skill, enabled: boolean) => void;
  /** A last column of row actions (edit, delete); the column is left out when absent. */
  rowActions?: (skill: Skill) => ReactNode;
  /** The skills to list, in any order. */
  skills: readonly Skill[];
  /** Extra tags after a skill's name, such as where the skill is managed. */
  tags?: (skill: Skill) => ReactNode;
}

function columns({
  host,
  onOpen,
  onToggle,
  rowActions,
  tags,
}: Omit<SkillTableProps, "skills">): Column<Skill>[] {
  const base: Column<Skill>[] = [
    {
      key: "name",
      label: "Skill",
      cell: (skill) => (
        <Cluster gap={2}>
          <Button variant="ghost" size="sm" onClick={() => onOpen(skill)}>
            <Text mono strong>
              {skill.name}
            </Text>
          </Button>
          {!skill.enabled && <Tag>Disabled</Tag>}
          {tags?.(skill)}
        </Cluster>
      ),
    },
    { key: "description", label: "Description", cell: (skill) => skill.description },
    {
      key: "risk",
      label: "Risk",
      hint: RISK_EXPLANATION,
      cell: (skill) => <StatusPill size="sm" meta={riskMeta(skill.risk)} />,
    },
    {
      key: "timeout",
      label: "Timeout",
      numeric: true,
      hint: "How long one run may take before the agent stops it.",
      cell: (skill) => `${skill.timeout_secs} s`,
    },
    {
      key: "applies",
      label: "Applies to",
      hint: "The checks this skill is suggested for. A skill that names none can be suggested for any check.",
      cell: (skill) =>
        skill.applies_to.length > 0 ? (
          <Cluster gap={1}>
            {skill.applies_to.map((check) => (
              <Tag key={check}>{check}</Tag>
            ))}
          </Cluster>
        ) : (
          <Text muted>Any check</Text>
        ),
    },
    {
      key: "last_used",
      label: "Last used",
      hint: `When Domovoi last sent this skill to the agent on ${host}.`,
      cell: (skill) =>
        skill.last_used ? <RelativeTime at={skill.last_used} /> : <Text muted>Never</Text>,
    },
    {
      key: "enabled",
      label: "Enabled",
      hint: `A disabled skill stays on ${host} but Domovoi never runs or suggests it.`,
      cell: (skill) => (
        <Switch
          label={<span className="visually-hidden">{`Enable ${skill.name}`}</span>}
          checked={skill.enabled}
          onChange={(enabled) => onToggle(skill, enabled)}
        />
      ),
    },
  ];
  if (rowActions) {
    base.push({ key: "actions", label: "Actions", hideLabel: true, cell: rowActions });
  }
  return base;
}

/** The skills of one host as a table that stacks into cards on a narrow screen. */
export function SkillTable({ skills, ...rest }: SkillTableProps) {
  return (
    <Stack gap={3}>
      <DataTable
        caption={`Skills on ${rest.host}`}
        columns={columns(rest)}
        rows={sortByRisk(skills)}
        rowKey={(skill) => skill.name}
        rowTone={(skill) => (skill.risk === "destructive" ? riskMeta(skill.risk).tone : undefined)}
        rowState={(skill) => (skill.enabled ? undefined : "quiet")}
      />
    </Stack>
  );
}
