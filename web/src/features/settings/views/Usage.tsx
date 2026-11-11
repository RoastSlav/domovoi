import type { Settings } from "../../../api/types";
import { Stack, Text } from "../../../components";
import styles from "../settings.module.css";

/** Model spending this month, against the budget when there is one. */
export function Usage({ settings }: { settings: Settings }) {
  const { usage, triage } = settings;
  const budget = triage.monthly_budget_usd;
  const spent = `$${usage.cost_usd.toFixed(2)}`;
  const tokens = `${usage.tokens_in.toLocaleString()} tokens in, ${usage.tokens_out.toLocaleString()} out`;
  const line =
    budget != null
      ? `${spent} of $${budget.toFixed(2)} this month · ${tokens}`
      : `${spent} this month, no budget set · ${tokens}`;
  return (
    <Stack gap={2}>
      <Text block>{line}</Text>
      {budget != null && budget > 0 && (
        <meter
          className={styles.meter}
          min={0}
          max={budget}
          high={budget * 0.8}
          value={Math.min(usage.cost_usd, budget)}
          aria-label="Budget used this month"
        >
          {spent}
        </meter>
      )}
    </Stack>
  );
}
