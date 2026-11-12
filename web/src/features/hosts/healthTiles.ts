import type { Fact } from "../../api/types";
import type { Tone } from "../../design/tone";
import { findFact, numberOf, shown, toneAt, withUnit } from "./factValue";

/** One number of the health row. */
export interface HealthTile {
  /** When the fact behind it was observed. */
  at: string;
  hint?: string | undefined;
  label: string;
  /** Longer text for a tooltip on the hint: every sensor, for the temperature. */
  tip?: string | undefined;
  tone?: Tone | undefined;
  value: string;
}

/** The health tiles read from `facts`, and the keys they used. */
export function healthOf(facts: readonly Fact[]): { tiles: HealthTile[]; used: Set<string> } {
  const used = new Set<string>();
  const tiles: HealthTile[] = [];
  const take = (...keys: string[]): Fact | undefined => {
    const found = findFact(facts, ...keys);
    if (found) {
      used.add(found.key);
    }
    return found;
  };

  const cpu = take("cpu.usage", "cpu.usage_pct");
  if (cpu && !Number.isNaN(numberOf(cpu))) {
    const value = numberOf(cpu);
    tiles.push({
      label: "CPU usage",
      value: `${shown(value)} %`,
      tone: toneAt(value, 75, 90),
      at: cpu.observed_at,
    });
  }

  const [one, five, fifteen] = [
    take("load.avg1", "load.1"),
    take("load.avg5", "load.5"),
    take("load.avg15", "load.15"),
  ];
  if (one) {
    const rest = [five && `5 min ${five.value}`, fifteen && `15 min ${fifteen.value}`];
    tiles.push({
      label: "Load",
      value: one.value,
      hint: rest.filter(Boolean).join(" · ") || undefined,
      at: one.observed_at,
    });
  }

  const [total, free, freePct] = [
    take("memory.total", "memory.total_gib"),
    take("memory.available", "memory.available_gib"),
    take("memory.available_pct"),
  ];
  if (free && !Number.isNaN(numberOf(free))) {
    const known = total && !Number.isNaN(numberOf(total)) ? numberOf(total) : undefined;
    const usedGib = known === undefined ? undefined : known - numberOf(free);
    let pct: number | undefined;
    if (!Number.isNaN(numberOf(freePct))) {
      pct = 100 - numberOf(freePct);
    } else if (known !== undefined && usedGib !== undefined) {
      pct = (usedGib / known) * 100;
    }
    tiles.push({
      label: "Memory",
      value: usedGib === undefined ? `${withUnit(free)} free` : `${shown(usedGib)} GiB used`,
      hint: known === undefined || !total ? undefined : `of ${withUnit(total)}`,
      tone: pct === undefined ? undefined : toneAt(pct, 85, 95),
      at: free.observed_at,
    });
  }

  const swap = take("swap.used_pct");
  if (swap && !Number.isNaN(numberOf(swap))) {
    const value = numberOf(swap);
    tiles.push({
      label: "Swap",
      value: `${shown(value)} %`,
      hint: "in use",
      tone: toneAt(value, 50, 80),
      at: swap.observed_at,
    });
  }

  const uptime = take("system.uptime");
  const days = uptime ? undefined : take("system.uptime_days");
  const up = uptime ?? days;
  if (up) {
    tiles.push({
      label: "Uptime",
      value: uptime ? uptime.value : `${up.value} days`,
      at: up.observed_at,
    });
  }

  const sensors = facts.filter(
    (fact) => fact.key.startsWith("temperature.") && !Number.isNaN(numberOf(fact)),
  );
  for (const sensor of sensors) {
    used.add(sensor.key);
  }
  const hottest = sensors.reduce<Fact | undefined>(
    (best, sensor) => (best === undefined || numberOf(sensor) > numberOf(best) ? sensor : best),
    undefined,
  );
  if (hottest) {
    tiles.push({
      label: "Temperature",
      value: `${shown(numberOf(hottest))} ${hottest.unit ?? "°C"}`,
      hint: hottest.label,
      tip: sensors.map((sensor) => `${sensor.label} ${withUnit(sensor)}`).join(", "),
      tone: toneAt(numberOf(hottest), 80, 90),
      at: hottest.observed_at,
    });
  }
  return { tiles, used };
}
