import type { Settings } from "../../../api/types";
import { Stack, StatTile, type KeyValue } from "../../../components";
import styles from "../settings.module.css";

const dollars = (amount: number): string => `$${amount.toFixed(2)}`;

/** The budget when one is set and can be measured against. */
export function budgetOf(settings: Settings): number | undefined {
  const budget = settings.triage.monthly_budget_usd;
  return budget != null && budget > 0 ? budget : undefined;
}

/** The month's model spending and token use as rows for a key and value list. */
export function usageRows(settings: Settings): KeyValue[] {
  const { usage, triage } = settings;
  const budget = triage.monthly_budget_usd;
  const spent = dollars(usage.cost_usd);
  return [
    {
      label: "Spent",
      value:
        budget != null
          ? `${spent} of ${dollars(budget)} this month`
          : `${spent} this month, no budget set`,
    },
    {
      label: "Tokens",
      value: `${usage.tokens_in.toLocaleString()} in, ${usage.tokens_out.toLocaleString()} out`,
    },
  ];
}

/** A bar of the budget used so far; nothing when there is no budget to measure against. */
export function BudgetMeter({ settings }: { settings: Settings }) {
  const budget = budgetOf(settings);
  if (budget === undefined) {
    return null;
  }
  const { cost_usd: spent } = settings.usage;
  return (
    <meter
      className={styles.meter}
      min={0}
      max={budget}
      high={budget * 0.8}
      value={Math.min(spent, budget)}
      aria-label="Budget used this month"
    >
      {dollars(spent)}
    </meter>
  );
}

/** Model spending this month and tokens used, as tiles, with the budget bar under them. */
export function Usage({ settings }: { settings: Settings }) {
  const { usage } = settings;
  const budget = budgetOf(settings);
  const used = budget === undefined ? undefined : Math.round((usage.cost_usd / budget) * 100);
  return (
    <Stack gap={3}>
      <div className={styles.tilesBox}>
        <div className={styles.tiles}>
          <StatTile label="Spent this month" value={dollars(usage.cost_usd)} />
          <StatTile
            label="Monthly budget"
            value={budget === undefined ? "None" : dollars(budget)}
            hint={used === undefined ? "No limit is set" : `${used}% used`}
          />
          <StatTile label="Tokens in" value={usage.tokens_in.toLocaleString()} />
          <StatTile label="Tokens out" value={usage.tokens_out.toLocaleString()} />
        </div>
      </div>
      <BudgetMeter settings={settings} />
    </Stack>
  );
}
