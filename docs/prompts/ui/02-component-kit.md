# UI 02 · The component kit: feedback, hints, states and building blocks

Branch: `ui-kit`. PR: `feat(web): …`. Needs 01. API changes only if the error details
need it (step 2).

After this part the kit exists, is documented and is shown in `/dev/design`, and the
existing pages use the new feedback primitives where it is a drop-in swap. Page layouts
are redesigned in 03–06.

Kit location: `web/src/components/`, one component per file (plus its CSS module), all
exported from `web/src/components/index.ts`. Every component has a `///`-style JSDoc
line saying what it is for, and an entry in `web/DESIGN.md` ("Use it for… Do not use it
for…").

## Steps

1. **Status display.**
   - `StatusPill({ meta, size?, showIcon? })`: renders a `StatusMeta` from `tone.ts` as a
     pill (soft tint, tone text, dot or icon, label) with the hint as its tooltip.
     Convenience wrappers are fine (`<VerdictPill value=…/>`), but they only call
     `tone.ts`. Replace `Badge` everywhere and delete it.
   - `Tag({ tone?, children })`: a neutral or toned chip for non-status labels (host
     kind, "maintenance", "hub-managed", source of a setting).
   - `Count({ tone, value, label })`: the filled number for nav items and filter chips;
     its accessible name says what it counts ("3 waiting").
   - `ToneStripe`: a utility class or prop for the 3 px left stripe on rows and cards.

2. **Errors and messages.**
   - `web/src/design/messages.ts`: `describeError(error: unknown, context?: string):
     { title: string; detail?: string; retryAfter?: number; issues?: Issue[] }`.
     Covers every hub code in the brief, network failure (`TypeError` from fetch), abort
     and timeout, and unknown errors. `context` is a short verb phrase
     ("approve the action") so titles read "Could not approve the action". The server
     message becomes `detail` when it differs from the title. Check how
     `web/src/api/types.ts` builds `ApiFailure` (`status`, `code`, `message`,
     `retryAfter`, `issues`).
   - If an important failure only reaches the UI as a generic `bad_request` or
     `conflict` without a usable message, improve the hub's message for it in
     `crates/domovoi-hub/src/api/error.rs` or the handler (allowed by the brief; add a
     test).
   - `InlineMessage({ tone, title, children, action? })`: a toned box inside a form or
     panel, `role="alert"` for `danger`, `role="status"` otherwise.
   - `Banner({ tone, title, children, action?, onDismiss? })`: full-width, top of the
     main area. `UpdateBanner` and `SetupBanner` move onto it in 03.

3. **Toasts.**
   - `ToastProvider` (in `App.tsx`), `useToast()` returning
     `{ success, error, info, dismiss }`. A toast has a title, optional detail, optional
     action (`{ label, onClick }`, for Undo, Retry, View), a tone and an icon.
   - Success and info auto-dismiss after ~4 s (paused on hover and focus); errors stay
     until dismissed. At most 3 visible; newest on top. Bottom-right on desktop,
     bottom-centre above the safe area on phones. One polite `aria-live` region (errors
     assertive). Esc dismisses the focused toast. Enter/leave motion uses the motion
     tokens.
   - `useActionFeedback` (in `web/src/design/` or `components/`): wraps a TanStack
     mutation so every caller declares its messages in one place:
     ```ts
     const ack = useActionFeedback(useAck(id), {
       success: "Incident acknowledged",
       error: "acknowledge the incident",      // context for describeError
       undo?: () => unack.mutate(),            // optional Undo action
     });
     ```
     It shows the success toast, or an error toast via `describeError` (unless the
     caller renders the error inline, `errorDisplay: "inline"`), and exposes
     `isPending` for buttons.
   - Write the "where does a message go" table from the brief into `DESIGN.md`.

4. **Hints.**
   - `Tooltip({ content, children, side? })`: hover after ~400 ms, focus at once, Esc
     closes, follows `aria-describedby`, flips to stay on screen, not shown for pure
     touch input. Positioning library or CSS anchor positioning with a fallback: your
     call per the brief.
   - `InfoTip({ title?, children, learnMore? })`: a 16 px info button opening a small
     popover (native Popover API or `<dialog>`-free popover with focus handling), closes
     on Esc and outside click, returns focus. `learnMore` reuses the book base URL from
     `PageIntro`.
   - `Button` gains: `loading` (spinner, keeps width, `aria-busy`), `icon` (leading
     `IconName`), `disabledReason` (renders the button `aria-disabled` and focusable with
     the reason as tooltip and `aria-describedby`; clicks do nothing), `size`
     (`sm | md`), variants `primary | secondary | danger | ghost`. `IconButton({ icon,
     label, … })` requires `label`, which is both its accessible name and its tooltip.
   - `RelativeTime` shows the absolute local time as a tooltip; `CopyBlock` and a new
     `CopyButton` confirm "Copied" (inline swap for 1.5 s plus `aria-live`).
   - Turn on the ESLint ban on JSX `title` in `src/features` and `src/app` as an error,
     and replace every `title=` (about 70 on `main`) with `Tooltip`, visible text or
     nothing, whichever is right.

5. **Loading, empty, error.**
   - `Loading({ label? })` (`role="status"`, delayed ~300 ms so fast loads do not
     flash) and `Skeleton` shapes (`line`, `block`, `table rows`) that reserve the final
     layout's space.
   - `LoadError({ error, onRetry, context })`: `describeError` text and a Retry button
     calling `refetch`, with `role="alert"`.
   - `EmptyState({ icon?, title, children, action? })`: says why it is empty and offers
     the next step.
   - `QueryBoundary({ query, loading?, empty?, isEmpty?, context, children })`: renders
     loading → error → empty → content for a TanStack query, so pages cannot forget a
     state. Use it in new code; parts 04–06 convert pages to it.

6. **Building blocks.**
   - Layout: `Stack({ gap })`, `Cluster({ gap, justify, wrap })`, `Grid({ min })`,
     `PageHeader({ title, intro?, docs?, actions?, meta? })` (replaces `.pageHead` +
     `PageIntro`; `meta` is a row of pills/tags under the title), `Section({ title,
     description?, actions?, children })`, `Card({ tone?, interactive?, children })`.
   - Data: `DataTable` (semantic `<table>`, sticky header, row tone stripe, optional
     row link where the whole row is clickable but the link stays the accessible
     element, responsive "stacked cards" mode below a breakpoint via CSS container
     queries, skeleton rows, empty slot), `KeyValueList` (definition list for details),
     `FilterChips` (toggle chips with optional counts and tone, single or multi select,
     a `radiogroup`/`group` with labels).
   - Forms: `Field({ label, hint?, error?, required?, children })` wiring `id`,
     `aria-describedby` and `aria-invalid`; `TextInput`, `Textarea`, `Select`,
     `Checkbox`, `Switch`, `SegmentedControl`, `NumberInput`; `FormErrorSummary({
     issues })` mapping hub `Issue` paths to field ids with links; `SubmitBar`
     (sticky on long forms: "3 unsaved changes", Discard, Save with `loading`).
   - Overlays: `Dialog` on native `<dialog>` (focus trap, Esc, `aria-labelledby`,
     restores focus, scroll lock); `ConfirmDialog({ title, consequence, confirmLabel,
     tone, requireText? })` where `requireText` asks to type a name for very
     destructive actions; `Menu` (actions menu behind a "More" `IconButton`, roving
     focus); `Popover` used by `InfoTip` and `Menu`. Keep `PasswordPrompt` on top of
     `Dialog`.
   - `Tabs`: arrow keys, Home and End select **and focus** the new tab (roving
     `tabIndex`), with an optional count per tab.
   - `Progress` (indeterminate bar for "running"), `Spinner` (inline, sm).
   - `Kbd` for keyboard hints in tooltips if you add any shortcut.

7. **Catalogue.** Every component above appears in `/dev/design` in every variant,
   state (default, hover via a class, focus, disabled with reason, loading, error) and
   tone, in both themes. Group it like `DESIGN.md`.

8. **Drop-in adoption.** Without redesigning pages: replace `Badge` with `StatusPill`,
   bare "Loading…" paragraphs with `Loading`, `EmptyState`-as-error with `LoadError`,
   and silent mutations with `useActionFeedback` (ack, mute, unmute, dismiss, retry
   triage, approve, deny, revoke, save note, delete note, import notes, send digest,
   save checks, save Docker choice, create invite, add rule, delete rule, settings save
   and apply). Keep inline errors where the control stays visible and the user must fix
   input.

9. **`DESIGN.md`**: fill in "Components" (name, use it for, do not use it for, one
   import line), "Messaging" (the table and voice rules, with five good and five bad
   examples), "Hints" (which hint for which case).

## Tests

- `messages.test.ts`: every hub code and network failure produces a non-empty title
  with the context; `retryAfter` and `issues` pass through; raw codes never appear in
  the title.
- Toasts: success auto-dismisses (fake timers) and pauses on hover; error stays; Undo
  calls its handler; the live region announces.
- `useActionFeedback`: success and error toasts; `errorDisplay: "inline"` shows none.
- Tooltip: appears on focus, hides on Esc, is linked by `aria-describedby`; a disabled
  `Button` with `disabledReason` is focusable, does not fire `onClick`, and exposes the
  reason.
- Tabs keyboard test (arrows, Home, End move focus).
- `Dialog`/`ConfirmDialog`: focus moves in and returns; Esc closes; `requireText` keeps
  the confirm disabled until typed.
- `Field` + `FormErrorSummary`: an issue path marks the right field invalid and the
  summary link focuses it.
- `QueryBoundary`: each of the four states.
- Existing page tests pass.

## Done when

- The kit covers every pattern the pages need; feature code needs no new styled
  primitives.
- Every mutation in the app reports success or failure in words; no `title=` remains;
  no page shows "Loading…" without `role="status"` or an error as an empty list.
- `/dev/design` shows the whole kit; `DESIGN.md` documents it.

## Out of scope

- Page layouts, navigation and the shell (03–06).
