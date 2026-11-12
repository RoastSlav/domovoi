import { useId, type ReactNode } from "react";
import type { Skill } from "../../api/types";
import {
  Button,
  Cluster,
  DataTable,
  RelativeTime,
  StatusPill,
  Switch,
  Tag,
  Text,
  type Column,
} from "../../components";
import { riskMeta } from "../../design/tone";
import { sortByRisk } from "./skillText";
import styles from "./skills.module.css";

/** What the table needs from its owner: what to do with a skill, and what to add to its rows. */
export interface SkillTableProps {
  /** The host the skills belong to; it names the tables and the switches' explanation. */
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
  rowActions,
  onToggle,
  tags,
}: Omit<SkillTableProps, "skills">): Column<Skill>[] {
  const base: Column<Skill>[] = [
    {
      key: "name",
      label: "Skill",
      width: "16rem",
      cell: (skill) => (
        <Cluster gap={2}>
          <Button variant="ghost" size="sm" className={styles.name} onClick={() => onOpen(skill)}>
            <Text mono strong>
              {skill.name}
            </Text>
          </Button>
          {!skill.enabled && <Tag>Disabled</Tag>}
          {tags?.(skill)}
        </Cluster>
      ),
    },
    {
      key: "description",
      label: "Description",
      truncate: "clamp2",
      tip: (skill) => skill.description,
      cell: (skill) => skill.description,
    },
    {
      key: "timeout",
      label: "Timeout",
      width: "6rem",
      numeric: true,
      hint: "How long one run may take before the agent stops it.",
      cell: (skill) => `${skill.timeout_secs} s`,
    },
    {
      key: "applies",
      label: "Applies to",
      width: "14rem",
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
      width: "8rem",
      hint: `When Domovoi last sent this skill to the agent on ${host}.`,
      cell: (skill) =>
        skill.last_used ? <RelativeTime at={skill.last_used} /> : <Text muted>Never</Text>,
    },
    {
      key: "enabled",
      label: "Enabled",
      width: "6rem",
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
    base.push({
      key: "actions",
      label: "Actions",
      hideLabel: true,
      width: "5rem",
      cell: rowActions,
    });
  }
  return base;
}

/** One risk level's skills: its pill and what it allows, then the skills in a bordered table. */
function RiskGroup({
  risk,
  skills,
  ...rest
}: { risk: string; skills: readonly Skill[] } & Omit<SkillTableProps, "skills">) {
  const id = useId();
  const meta = riskMeta(risk);
  return (
    <section aria-labelledby={id} className={styles.group}>
      <div className={styles.groupHead}>
        <h2 id={id} className={styles.groupTitle}>
          <StatusPill meta={meta} />
        </h2>
        <Text muted size="sm">
          {meta.hint}
        </Text>
      </div>
      <DataTable
        caption={`${meta.label} skills on ${rest.host}`}
        columns={columns(rest)}
        layout="fixed"
        rows={skills}
        rowKey={(skill) => skill.name}
        onRowClick={rest.onOpen}
        rowTone={(skill) => (skill.risk === "destructive" ? meta.tone : undefined)}
        rowState={(skill) => (skill.enabled ? undefined : "quiet")}
      />
    </section>
  );
}

/**
 * The skills of one host grouped by risk, the harmless first, each group a pill and a
 * bordered table that stacks into cards on a narrow screen. A row opens the skill. Every
 * group uses the same fixed column widths so the columns line up from one table to the next;
 * a long description clamps to two lines.
 */
export function SkillTable({ skills, ...rest }: SkillTableProps) {
  const groups = new Map<string, Skill[]>();
  for (const skill of sortByRisk(skills)) {
    groups.set(skill.risk, [...(groups.get(skill.risk) ?? []), skill]);
  }
  return (
    <div className={styles.groups}>
      {[...groups].map(([risk, list]) => (
        <RiskGroup key={risk} risk={risk} skills={list} {...rest} />
      ))}
    </div>
  );
}
