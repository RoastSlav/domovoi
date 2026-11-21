import { usePolicyOverview } from "../../api/hooks/policy";

/**
 * Names the zone for a sentence about times.
 * @param zone The hub's time zone, if it said one.
 */
export function zoneLabel(zone: string | undefined): string {
  return zone ? zone : "the hub's time zone";
}

/**
 * The time zone the hub reads time windows in.
 * @returns The zone, or undefined while loading or when the hub did not say.
 */
export function useTimeWindowZone(): string | undefined {
  const zone = usePolicyOverview().data?.time_window_zone;
  return zone ? zone : undefined;
}
