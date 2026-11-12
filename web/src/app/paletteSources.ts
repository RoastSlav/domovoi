import { useEffect, useState } from "react";
import { INCIDENT_PAGE, useIncidents } from "../api/hooks/incidents";
import type { IncidentSummary } from "../api/types";
import type { SectionSpec } from "../features/settings/sections";
import type { PaletteItem } from "../components";
import { incidentStatusMeta, severityMeta } from "../design/tone";

/** How many incidents the palette lists. */
export const PALETTE_INCIDENTS = INCIDENT_PAGE;

const RANK: Record<string, number> = { critical: 0, warning: 1, info: 2 };

/** Incidents most urgent first: severity, then the most recent to fire. */
export function mostUrgentFirst(list: readonly IncidentSummary[]): IncidentSummary[] {
  return [...list].sort(
    (a, b) =>
      (RANK[a.severity] ?? 3) - (RANK[b.severity] ?? 3) ||
      b.last_firing_at.localeCompare(a.last_firing_at),
  );
}

/**
 * The open incidents as palette items, fetched only while the palette is open: a title, then
 * its severity and state at the right. Opens the incident's page.
 */
export function useIncidentItems(open: boolean, go: (to: string) => () => void): PaletteItem[] {
  const incidents = useIncidents({ status: "open" }, open);
  const first = incidents.data?.pages[0]?.items ?? [];
  return mostUrgentFirst(first)
    .slice(0, PALETTE_INCIDENTS)
    .map((incident) => ({
      id: `incident-${incident.id}`,
      label: incident.title,
      icon: "alert",
      hint: `${severityMeta(incident.severity).label} · ${incidentStatusMeta(incident.status).label}`,
      keywords: ["incident", incident.check, incident.target ?? "", incident.severity],
      onSelect: go(`/incidents/${encodeURIComponent(incident.id)}`),
    }));
}

/**
 * One palette item per settings section, found by its title, its sentence and the labels of
 * its fields ("quiet hours" finds Notifications). The section list is its own download,
 * fetched when the palette first opens, so the first page load does not carry it.
 */
export function useSettingsItems(open: boolean, go: (to: string) => () => void): PaletteItem[] {
  const [sections, setSections] = useState<SectionSpec[]>([]);
  useEffect(() => {
    if (!open || sections.length > 0) {
      return;
    }
    let live = true;
    void import("../features/settings/sections").then((found) => {
      if (live) {
        setSections(found.SECTIONS);
      }
    });
    return () => {
      live = false;
    };
  }, [open, sections.length]);
  return sections.map((section) => ({
    id: `setting-${section.id}`,
    label: `${section.title} settings`,
    icon: "gear",
    keywords: [
      section.title,
      section.intro ?? "",
      ...section.fields({}).map((field) => field.label),
    ],
    onSelect: go(`/settings/${section.id}`),
  }));
}
