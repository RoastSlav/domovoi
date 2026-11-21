import { useMemo } from "react";
import type { PolicyRule } from "../../api/types";
import { DataTable, Menu, Text, Tooltip, type Column } from "../../components";
import { ORDER_NOTE } from "./policyText";
import { OutcomePill, RuleMarks } from "./RuleMarks";
import { conditionText, scopeText } from "./ruleText";
import styles from "./policy.module.css";

type HostNames = ReadonlyMap<string, string> | Record<string, string>;

function Conditions({ rule, zone }: { rule: PolicyRule; zone: string | undefined }) {
  const all = rule.conditions ?? [];
  const first = all[0];
  if (!first) return <Text muted>None</Text>;
  const rest = all.slice(1).map((c) => conditionText(c, zone));
  const label = <Text size="sm">{conditionText(first, zone)}</Text>;
  return (
    <span className={styles.wrapCell}>
      {all.length > 1 ? (
        <Tooltip content={rest.join(". ")}>
          <span>{label}</span>
        </Tooltip>
      ) : (
        label
      )}
      <Text muted size="xs">
        {`${all.length} ${all.length === 1 ? "condition" : "conditions"}`}
      </Text>
    </span>
  );
}

/**
 * The rules of a version or draft as a table. Order carries no meaning, and the table says so.
 * In `edit` mode each row gets a menu to edit, duplicate or remove the rule; the callbacks
 * receive the rule id.
 */
export function RulesTable({
  rules,
  hostNames,
  zone,
  mode = "view",
  onEdit,
  onDuplicate,
  onRemove,
  selectedId,
  caption = "Rules",
}: {
  caption?: string;
  hostNames: HostNames;
  mode?: "view" | "edit";
  onDuplicate?: (id: string) => void;
  onEdit?: (id: string) => void;
  onRemove?: (id: string) => void;
  rules: readonly PolicyRule[];
  /** The rule being edited; its row is marked. */
  selectedId?: string | undefined;
  zone: string | undefined;
}) {
  const columns = useMemo<Column<PolicyRule>[]>(() => {
    const base: Column<PolicyRule>[] = [
      {
        key: "rule",
        label: "Rule",
        cell: (rule) => (
          <span className={styles.wrapCell}>
            <Text strong mono>
              {rule.id}
            </Text>
            {rule.note && (
              <Text muted size="sm">
                {rule.note}
              </Text>
            )}
            <RuleMarks rule={rule} />
          </span>
        ),
      },
      {
        key: "scope",
        label: "Applies to",
        cell: (rule) => <span className={styles.wrapCell}>{scopeText(rule, hostNames)}</span>,
      },
      {
        key: "conditions",
        label: "Conditions",
        cell: (rule) => <Conditions rule={rule} zone={zone} />,
      },
      {
        key: "outcome",
        label: "Outcome",
        width: "11rem",
        cell: (rule) => <OutcomePill outcome={rule.outcome} />,
      },
    ];
    if (mode !== "edit") return base;
    return [
      ...base,
      {
        key: "actions",
        label: "Actions",
        hideLabel: true,
        width: "4rem",
        cell: (rule) => (
          <Menu
            label={`Actions for ${rule.id}`}
            items={[
              { label: "Edit", icon: "edit", onSelect: () => onEdit?.(rule.id) },
              { label: "Duplicate", icon: "copy", onSelect: () => onDuplicate?.(rule.id) },
              {
                label: "Remove",
                icon: "trash",
                tone: "danger",
                onSelect: () => onRemove?.(rule.id),
              },
            ]}
          />
        ),
      },
    ];
  }, [hostNames, zone, mode, onEdit, onDuplicate, onRemove]);

  return (
    <>
      <Text muted block>
        {ORDER_NOTE}
      </Text>
      <DataTable
        caption={caption}
        columns={columns}
        rows={rules}
        rowKey={(rule) => rule.id}
        rowTone={(rule) =>
          rule.id === selectedId
            ? "info"
            : rule.locked && rule.outcome === "deny"
              ? "danger"
              : undefined
        }
        empty="No rules yet"
      />
    </>
  );
}
