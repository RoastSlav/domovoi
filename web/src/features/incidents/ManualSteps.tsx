import { useState } from "react";
import { Checkbox, Section, Stack } from "../../components";

function storageKey(incidentId: string): string {
  return `incident-steps:${incidentId}`;
}

/** The ticked steps saved for the incident; nothing when storage is empty or blocked. */
function readTicked(incidentId: string): number[] {
  try {
    const saved: unknown = JSON.parse(sessionStorage.getItem(storageKey(incidentId)) ?? "[]");
    return Array.isArray(saved) ? saved.filter((n): n is number => typeof n === "number") : [];
  } catch {
    return [];
  }
}

/**
 * The steps triage suggests doing by hand, as a checklist. Ticks are the person's own
 * bookkeeping: kept in this browser tab for the incident and never sent to the hub.
 */
export function ManualSteps({ incidentId, steps }: { incidentId: string; steps: string[] }) {
  const [ticked, setTicked] = useState(() => readTicked(incidentId));
  if (steps.length === 0) {
    return null;
  }
  const toggle = (index: number, on: boolean): void => {
    const next = on ? [...ticked, index] : ticked.filter((other) => other !== index);
    setTicked(next);
    try {
      sessionStorage.setItem(storageKey(incidentId), JSON.stringify(next));
    } catch {
      // Private windows and blocked site data: the ticks last until the page is left.
    }
  };
  return (
    <Section
      title="Check by hand"
      description="Written by the model; Domovoi does not run these. Ticks are only for you, in this tab."
    >
      <Stack gap={2}>
        {steps.map((step, index) => (
          <Checkbox
            key={`${step}-${index}`}
            label={step}
            checked={ticked.includes(index)}
            onChange={(on) => toggle(index, on)}
          />
        ))}
      </Stack>
    </Section>
  );
}
