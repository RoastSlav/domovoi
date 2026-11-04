# 03 · Fix: resolved incidents still show "Action needed" in the list

## Goal

Anywhere an incident is listed or carded, a resolved incident shows a **Resolved** badge.
The triage verdict stays visible as secondary text.

## What is already known (verified)

- The list row badge is `incident.verdict ?? incident.severity`
  (`web/src/features/incidents/IncidentListPage.tsx`, `IncidentRow`). The verdict
  (`ACTION_NEEDED`) is kept after the incident resolves, so the badge still says
  "Action needed" while the meta line says "resolved".
- `web/src/features/hosts/HostDetailPage.tsx`, `RecentIncidents`, has the same pattern.
- The detail page (`IncidentCard.tsx`) shows the verdict badge followed by " Resolved",
  which is why opening the incident looks correct.
- The backend is right: keep the verdict as history. The fix is in the web UI only.

## Read these files

- `web/src/components/Badge.tsx` and `web/src/components/Badge.module.css`
- `web/src/features/incidents/IncidentListPage.tsx`
- `web/src/features/incidents/IncidentCard.tsx`
- `web/src/features/hosts/HostDetailPage.tsx`
- `web/src/features/incidents/incidents.test.tsx`, `web/src/test/fixtures.ts`

## Steps

1. In `Badge.tsx` add `resolved: "Resolved"` to `LABELS` and `resolved: "resolved"` to
   `TONES`. In `Badge.module.css` add `.badge[data-tone="resolved"]` using
   `background: var(--fixed)`.
2. Add `web/src/features/incidents/badge.ts`:
   ```ts
   import type { IncidentSummary } from "../../api/types";
   /** The badge an incident shows: Resolved once it resolved, else its verdict or severity. */
   export function incidentBadge(i: Pick<IncidentSummary, "status" | "verdict" | "severity">): string {
     return i.status === "resolved" ? "resolved" : (i.verdict ?? i.severity);
   }
   ```
3. Use `incidentBadge(incident)` in `IncidentRow` and in `RecentIncidents`.
4. In `IncidentRow`, when the incident is resolved and has a verdict, append
   ` · triage said ${badgeLabel(verdict)}` to the meta line (import `badgeLabel`).
5. In `IncidentCard.tsx`, when `card.resolved`, render `<Badge value="resolved" />`
   instead of the verdict badge and replace the `<span> Resolved</span>` with
   `<span className={styles.meta}>Triage said {badgeLabel(card.badge)}</span>`.

## Tests

In `incidents.test.tsx` add `resolved_incidents_show_a_resolved_badge`: the list returns
`incident({ status: "resolved", verdict: "ACTION_NEEDED" })`; expect the text "Resolved"
in the row and no element with the exact text "Action needed" as a badge (the meta line
says "triage said Action needed"). Update any existing assertion that breaks.

## Done when

- Lists, host pages and the detail card show "Resolved" for resolved incidents.
- Open incidents still show their verdict or severity.
- `npm run lint && npm run typecheck && npm test` pass.
