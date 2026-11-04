# 12 · Feature: explain what things are, in the UI

## Goal

Every page says in one or two sentences what it is for, with a "Learn more" link to the
docs, and the jargon (verdicts, statuses, notes, skills, approvals, ack, mute, digest,
follow-up) has short explanations where it appears. Web only.

## What is already known (verified)

- Pages: `web/src/features/{incidents,approvals,hosts,skills,notes,digest,settings}/*Page.tsx`
  and `IncidentDetailPage.tsx`. None of them explains itself; for example Notes only says
  "Write one, or import a folder of Markdown runbooks."
- The user docs are published at `https://RoastSlav.github.io/domovoi/` from
  `docs/book/src/` (mdBook: `concepts/incidents.md` → `concepts/incidents.html`).
- What each concept means (use these facts for the copy):
  - **Notes**: Markdown runbooks, per host or global. Triage searches them and quotes the
    relevant ones to the model, so write down how your setup works, what is normal, and
    how you fixed things before (`docs/book/src/concepts/how-it-works.md`,
    `crates/domovoi-engine/src/triage/context.rs` `notes()`).
  - **Skills**: scripts on the agent host, described by `skill.toml`. Read skills only
    look and triage may run them itself; write and destructive skills change things and
    only run after a person approves (`docs/book/src/guide/skills.md`).
  - **Approvals**: actions triage proposed, waiting for a human. Destructive ones need
    the skill name typed; they expire after an hour.
  - **Verdicts**: Action needed / Monitor / Expected / Fixed
    (`crates/domovoi-engine/prompts/triage_system.md`, "Verdicts").
  - **Statuses**: pending (seen once, waiting for confirmation), open, acked (someone
    is on it; reminders stop), muted (no reminders until a time), resolved.
  - **Ack** stops reminders; **Mute** silences until a time; **Follow-up** asks the
    model about this incident with read-only tools.
  - **Digest**: one daily summary of all hosts, sent at `notify.digest.time`.
  - **Timeline**: everything that happened to the incident, oldest first.

## Read these files

- `web/src/components/EmptyState.tsx`, `web/src/components/ui.module.css`
- `web/src/components/Badge.tsx`
- Each `*Page.tsx` listed above, and `IncidentControls.tsx`, `AskBox.tsx`

## Steps

1. Create `web/src/components/PageIntro.tsx`:
   ```tsx
   /** One or two sentences under a page heading, with an optional docs link. */
   export function PageIntro({ children, docs }: { children: ReactNode; docs?: string }) { ... }
   ```
   It renders `<p className={styles.intro}>{children} {docs && <a href={`${DOCS}${docs}`} target="_blank" rel="noreferrer">Learn more</a>}</p>`
   with `const DOCS = "https://RoastSlav.github.io/domovoi/";`. Add `.intro` (muted
   colour, max-width 70ch, margin-bottom 1rem) to `ui.module.css`.
2. Add a `<PageIntro>` right after the `<h1>` of each page, with these texts (keep them
   this short):
   - Incidents: "Problems Domovoi found on your hosts. Each one is triaged by the model
     and posted to chat." → `concepts/incidents.html`
   - Approvals: "Actions the model suggested. Nothing runs until you approve it; requests
     expire after an hour." → `concepts/actions.html`
   - Hosts: "Machines Domovoi watches. Each runs an agent that sends checks to this hub."
     → `concepts/hub-and-agent.html`
   - Skills: "Scripts an agent can run. Read skills only look, and triage may run them on
     its own. Write and destructive skills change things and need your approval."
     → `guide/skills.html`
   - Notes: "Runbooks in Markdown: how your setup works and how you fixed things before.
     Triage searches them and gives the relevant ones to the model." →
     `concepts/how-it-works.html`
   - Digest: "One summary of every host per day: what changed, trends and open
     incidents." → `concepts/digest.html`
   - Settings: "How this hub runs. Values set in hub.toml or the environment are shown
     but locked." → `reference/configuration.html`
3. Add a `title` tooltip to badges: in `Badge.tsx` add a `HINTS` map
   (`ACTION_NEEDED: "Something is broken or will break soon; act now."`,
   `MONITOR: "Off but not urgent; keep an eye on it."`,
   `EXPECTED: "Explained by something intended, like an update or a known condition."`,
   `FIXED: "Already recovered."`, `resolved: "The problem cleared."`) and render
   `title={HINTS[value]}` on the span.
4. In `IncidentControls.tsx` give the buttons `title`s: Ack "Stops reminders; the
   incident stays open until the problem clears.", Mute "No alerts or reminders until
   the time you pick.". In `AskBox.tsx` add a hint line under the label: "The model
   answers with read-only tools. It cannot change anything."
5. In `IncidentDetailPage.tsx`, give the Timeline section a hint: "Everything that
   happened to this incident, oldest first."
6. Check every link target exists in `docs/book/src/SUMMARY.md`; change a path if not.

## Tests

- `web/src/components/components.test.tsx`: `PageIntro` renders its text and a link
  with `target="_blank"` to the docs base plus the path.
- Update any page test that now finds two matching texts.

## Done when

- Every listed page has an intro, badges have tooltips, and Ack/Mute/Follow-up say what
  they do.
- `npm run lint && npm run typecheck && npm test` pass.
