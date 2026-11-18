# 49 — Capability Center: owner import, install wizard, host limits and helper placeholders

| | |
|---|---|
| Branch | `capability-center-and-install-wizard` |
| Milestone | U4 |
| Depends on (must be merged) | 37 (visibility profiles), 30 (operations UI: shell right slot, smoke test, vocabulary conventions), 25 (capability registry: `/capabilities` page, `GET /hosts/{name}/capabilities`, `Availability`, `parse_ceiling`), 42 (packages hub: every `/api/v1/packages*` endpoint), and through them 24, 29 and 41. All of 25, 30, 37, 41 and 42 are written but may not be merged when this brief is read: **every name taken from them is "introduced by prompt NN — verify in code before use"**. If any of them is missing, stop and report. |
| Can run in parallel with | 43, 48, 50, 51 (order-flexible, never concurrent). They share `web/src/app/{pages,routes,PageTitle}`, `web/src/design/*`, `web/src/api/{queryClient,types}.ts`, `web/DESIGN.md`, `VISIBILITY_PAGES` and the smoke mock; whichever runs second rebases on the first. |
| Produces (shared contracts) | (1) `GET /api/v1/hosts/{name}/ceiling` and `CeilingDto` (read-only; the only hub change). (2) Web: the Capability Center tab shell, the package, install-job and ceiling hooks, the install vocabularies (`INSTALL_TARGET_STATUSES`, `INSTALL_JOB_STATUSES`, `HOST_PACKAGE_STATUSES`, `KEY_STATES`, `FILE_STATUSES`, `PRIVILEGE_BOUNDARIES`, `CEILING_REPORTING`), `describeTargetFailure`, and the two **placeholder mount points** `HelperPlaceholder` and `HelperKeyPlaceholder` that the helper prompt replaces. |
| Consumes | 25: `/capabilities` page and `useCapabilities`, `CAPABILITY_STATES`, `capabilityText.ts`, `parse_ceiling`, `HubRegistry`. 42: the endpoints and DTOs of its section 6.9 (job status `running | succeeded | failed | partial`, step-up for `installation.start`). 41: `PackageFailureCode` words, `PackageStatus`, `KeyState`, `PrivilegeBoundary`. 37: `useVisibility`, `Visible`, `DetailsScope`, `visibilityAttrs`, `VisibilityPage`, `expectP0Everywhere`, rule 6.5. 30: `npm run smoke` and its mock hub. Existing kit and `design/` tables. |
| Launch gate | none. This prompt deliberately does **not** wait for the helper prompt (47, gated on R-3, R-9, R-10): the helper and key-enrollment views are built here as labelled placeholders only (section 2, decision 1). |
| Suggested model | medium (many small files; every decision is made below; the judgement is in honest wording and in never offering automation) |

## 1. Objective

Turn the read-only capability inventory of prompt 25 into the **Capability Center**, and give the owner the
screens to put a signed package on their hosts and to see what each host allows. Concretely, in the web UI
(plus one small read-only hub endpoint):

1. **Capability Center shell.** `/capabilities` becomes a page with tabs: **Inventory** (prompt 25's
   per-host list, mounted unchanged as the first tab), **Packages**, **Host limits** and **Helper**. The
   tab lives in the URL (`?tab=`).
2. **Owner package import.** Upload an owner-signed `<name>-<version>.tar.zst` and its detached
   `<name>-<version>.sig.toml` (multipart, prompt 42's `POST /api/v1/packages/import`). The screen shows
   the signature result the hub reports (signer label and key id, "checked by this hub") and says that
   each host checks it again against its own keys. Package list, package details, delete.
3. **Install wizard.** Plan → human authorization → per-host job progress, as one guided flow:
   choose package → choose hosts → review the plan the hub computed → authorize with your password and by
   typing the package name → watch each host. Per-host **retry** and **cancel**, and "check host" to see
   what is really installed. Uninstall uses the same flow. The wizard offers **no automation of any kind**:
   package installation is human-only.
4. **Host limits (the unattended ceiling), read-only.** For one host, what that host's own configuration
   allows to run unattended, exactly as the agent reported it, or "not reported". The hub cannot change it
   (D-03) and the screen has no control that suggests otherwise. This is the one place this prompt touches
   Rust: a read-only `GET /api/v1/hosts/{name}/ceiling` (section 6.1), because no endpoint exposes the
   ceiling today.
5. **Helper views and key-enrollment screen: labelled placeholders.** The helper's status, its signed
   safety attestation and helper-key enrollment have **no backend on this hub** (they arrive with the
   helper prompt, gated on R-3, R-9, R-10). This prompt builds the two views **only** as a
   clearly labelled "not available on this hub yet" placeholder: no data, no request, no invented DTO. Section
   6.9 lists exactly what the helper prompt must add later.
6. **Honest words.** Every status a package install can be in gets a label, a tone, a hint and an icon from
   the vocabularies of `web/src/design/`; every hub error code the packages endpoints add and every agent
   failure code gets a plain sentence in `messages.ts`.
7. **Visibility.** Every element of the new screens carries a category P0 to P3 (prompt 37's rule 6.5);
   `p0_visible_in_every_profile` covers them. A real-browser smoke check (prompt 30's `npm run smoke`) and
   `screens.mjs` pages cover layout, focus and colour contrast.

It does **not** change the wire, the agent, the package store, the install worker, any approval rule, any
migration, or the policy system. It never creates an install job on its own: the only caller of the
job-creating and retrying hooks is the confirm button of the authorization dialog (gate test, section 12).

## 2. Architectural context

Everything needed is here; the cited documents are background only.

**RW-26, D-11, D-16: who is trusted for packages.** The **agent** is the only authority over what is
installed on its host. It verifies a package against keys the owner wrote into that host's `agent.toml`.
There is no third-party registry in v1, and the hub **cannot add, replace or remove a key on any agent**. The
hub keeps a file-only key list (`[[packages.owner_keys]]` and `revoked` in `hub.toml`) used only to refuse
an import nobody could install and to show a signer label. The Capability Center therefore:

- shows the hub's owner-key list and revoked list **read-only** (no add, remove or rotate control);
- words the signature result as "checked by this hub" and never as "trusted by your hosts";
- never offers a way to change a key anywhere.

**Install is not enable.** "Installed" means the agent holds verified files. "Enabled" is a separate action
that does not exist yet, and package skills cannot run before the isolated skill runner exists. Nothing on
these screens says "enabled", "ready to use", "active and running" or "available" about an installed
package. The words are **installed** and **removed**.

**Human-only (04 §11.2, RW-26; INV-1).** An install or removal is authorized by an authenticated human with a
step-up: password plus typed package name, bound to the plan code the screen shows. There is no chat path,
no model tool, no policy path, no schedule, no "install everywhere" shortcut, no "update automatically".
The wizard has no switch, checkbox, schedule, "apply to all hosts" or "remember my approval" control. Retry
needs a fresh approval too (a new password entry and a typed name), because the hub creates a new
authorization for each attempt.

**Plan code (D-03 spirit, RW-11).** The hub computes a plan per host and a job hash over them. The screen
shows the first 8 hex characters of the job hash as `Plan xxxxxxxx` and submits the **full** hash the person
reviewed. If anything changed between review and confirm (a re-enrolled host key, a replaced artifact, a
changed signer) the hub answers `409 plan_changed` and the wizard sends the person back to the review. The
UI never recomputes or edits a hash.

**D-03: the host ceiling is host-local and read-only here.** `agent.toml [unattended]` bounds what runs
without a person. The agent summarises it (`ceiling_hash` and `ceiling_summary` in `hello`, wire 1.2); the hub
cannot modify or widen it; hub policy may only narrow it. The UI shows what the agent reported
(`CeilingSummary`, prompt 24, parsed by prompt 25's `parse_ceiling`) or **"not reported"**. Before the
autonomy prompt ships, no agent reports one, so "not reported" is the normal state today and it must read as
information, never as a fault and never as "everything is allowed". A summary the hub cannot read is
**unreadable**, shown as such, and treated as not permissive.

**D-29, RW-37, RW-39: why the helper views are placeholders.** The root-owned helper, its signed safety
attestation (`attest_safety` / `safety_attestation`), the owner's fingerprint comparison for pinning its key,
and the covered/uncovered management-path view all belong to the helper prompt (H1), which is gated and has
not run. This hub has no endpoint, table or DTO for any of it. Showing invented or empty helper data would
tell the owner something false about their hosts, so the screens say plainly that this hub does not collect it
yet. `helper_status`, `skill_runner_status` and `runs_as_root` do arrive in wire 1.2 `hello`, and prompt 25 reads
them, but this prompt adds **no** endpoint for them (that would be inventing a helper DTO): the inventory's
existing `root_agent` and `runner_installed` labels (prompt 25) are the only place they surface.

**RW-21 / D-10: profiles.** One component set, one category per element, never P0 hidden. Prompt 37 classified
only the operations screens; this prompt classifies its own with the same rule (section 6.6).

**Availability is not authorization (RW-10, prompt 25).** The plan's per-host `availability` is the registry's
answer for `agent.package.install`/`uninstall`. It explains whether the hub would offer the install there; it is
never an approval. The screens render it through prompt 25's `CAPABILITY_STATES` vocabulary and sentences.

**Decisions made here (do not revisit).**

1. **Helper and key enrollment are placeholders** (assignment decision): two views, a tab body and a page at
   `/capabilities/helper-key`, both labelled "Not available on this hub yet", with no request and no fake data.
2. **The Capability Center is tabs on prompt 25's page**, not a second page. The Inventory tab is prompt 25's
   content moved, not copied. `/capabilities` stays in the `Fleet` nav group; no new nav item.
3. **The wizard is a page route** (`/capabilities/install`), not a dialog, so a reload or a shared link keeps the
   package choice. Hosts and plan state are in memory. The password lives only in the authorization dialog.
4. **Job progress is a page** (`/capabilities/jobs/:id`) reached right after confirming, and from "Recent
   installs" on the Packages tab. It **polls** (the install job writes journal rows that no SSE topic carries):
   `JOB_POLL_MS = 3000` while any target is `queued`, `transferring` or `dispatched`; it stops when none is.
5. **Retry re-plans one host** with `POST /packages/jobs/plan` for that host (the job DTO carries no plan hash),
   then asks for the password and typed name again and submits the hash it just showed.
6. **The ceiling endpoint is added here** (prompt 25 says the view is this prompt's but exposes none): read-only,
   `GET` only, by host name like 25's `/hosts/{name}/capabilities`.
7. **A Settings section "Packages"** (two fields, `packages.enabled` and `packages.approval_window`) is added so
   the owner can turn installation on from the web, because prompt 42 adds the keys but no field. The key lists
   and size limits stay file-only; the Packages tab says so.
8. **No kit component is added.** Pages use the existing kit (`DropZone`, `Stepper`, `Tabs`, `DataTable`,
   `ConfirmDialog`, `PasswordInput`, `Field`, `KeyValueList`, `StatusPill`, `Tag`, `InlineMessage`, ...). Small
   feature-level components live under `src/features/`.

## 3. Repository context

Marks: **(existing, path:line)** verified in the checkout, **(introduced by NN — verify)**, **(new)**. Paths
are under `web/` unless they start with `crates/` or `docs/`. Verify each in step 1.

### 3.1 Web: what exists

- `src/app/nav.ts` **(existing)**: `NAV_GROUPS` (group `Fleet` holds Hosts, Skills, Logs). **25** adds a
  `Capabilities` item there; this prompt adds no nav item. `src/app/pages.ts` **(existing)**: `PAGES` map and
  `page()`; **25** adds `PAGES.capabilities`. `src/app/routes.tsx:75` **(existing)**: `/skills` route, inside
  `<Layout/>`; **25** adds `/capabilities`. `src/app/PageTitle.tsx:45` **(existing)**: `defaultDetail(section,
  rest)` with a `switch` per section; the section name comes from the nav item label.
- `src/app/AllHostsNote.tsx` **(existing)**: renders only while a host is selected, to say a page ignores the
  selector. `useSelectedHost()` in `src/app/HostContext.tsx` **(existing)**. `src/features/skills/ChooseHost.tsx`
  **(existing)**: the "choose a host" cards used by `/skills` under "All hosts" (its links go to
  `/skills?host=<name>`).
- `src/components/index.ts` **(existing)** is the only import point. Kit pieces used here: `PageHeader`,
  `Section`, `Card`, `Tabs` (controlled by `selected` and `onSelect`, `Tab.id`), `DataTable`, `DropZone`
  (`label`, `hint`, `accept`, `multiple`, `disabledReason`, `onFiles`; `DropZone.tsx:34`), `Stepper`
  (`steps[{id,label,state}]`, `label`; states `done|current|skipped|upcoming`; `Stepper.tsx:20`), `ConfirmDialog`
  (`requireText`, `onTyped`, `children`, `error`, `confirmDisabledReason`, `busy`; `ConfirmDialog.tsx:44`),
  `Dialog` (`Dialog.tsx:27`), `PasswordInput`, `Field`, `TextInput`, `Checkbox`, `KeyValueList`, `StatusPill`,
  `Tag`, `Count`, `InlineMessage`, `Banner`, `Progress`, `Skeleton`, `EmptyState`, `LoadError`,
  `QueryBoundary`, `Button` (`disabledReason`, `hint`, `loading`), `LinkButton`, `IconButton`, `CopyBlock`,
  `RelativeTime`, `Countdown`/`useRemaining`, `InfoTip`, `Tooltip`, `Cluster`, `Stack`, `Grid`, `Text`,
  `useActionFeedback`. `PasswordPrompt` exists but only asks for a password; the wizard needs password **and**
  typed text, so it uses `ConfirmDialog` with `requireText` and a password `Field` as `children`.
- `src/api/types.ts` **(existing)**: DTO aliases from `schema.d.ts`, `call<T>(request)` (`:143`) throws
  `ApiFailure` (`:97`), `defined(obj)` (`:160`), `Host = Schemas["HostDto"]` (`:32`; has `id` and `name`).
  `src/api/queryClient.ts:5` **(existing)**: `keys`. `src/api/hooks/hosts.ts` **(existing)**: `useHosts` (`:23`),
  `useHostNames` (`:32`). `src/api/hooks/notes.ts:90-115` **(existing)**: the multipart precedent, `importForm` +
  `bodySerializer: () => form` with a placeholder `body`. `src/features/notes/{ImportDialog,ImportResult}.tsx`
  and `noteImport.test.tsx` **(existing)**: the dialog, result summary and test style to copy.
- `src/design/status.ts` **(existing)**: `Table<K>`, `StatusMeta {label, tone, hint, icon}`, `lookup()` (an unknown
  value becomes a neutral meta carrying the raw text and "not known to this version of the UI"). `src/design/
  tone.ts` **(existing, 208 lines before 25 and 30 add to it)**: lookups plus `VOCABULARIES` (`:188`), which
  `tone.test.ts` walks. `src/design/fleetVocab.ts` **(existing)**: model for a vocabulary file.
  `src/design/icons.tsx` **(existing)**: closed `ICONS` map; `Icon name=`.
- `src/design/messages.ts` **(existing)**: `HUB_CODES` (`:29`) is the closed list of hub error codes,
  `CATALOGUE` (`:49`) maps each to `{reason, specific}`; an `ApiFailure` whose code is not in it reads as
  "The page did not understand what came back" (`reasonFor`). `messages.test.ts` runs `it.each(HUB_CODES)` with a
  `STATUS` map. **Prompt 42 adds fifteen error codes to the hub but no words to the web**: this prompt does.
- `src/features/settings/sections/{index,upkeep,types}.ts` **(existing)**: a section is one entry of a section
  list; a field is one `FieldSpec { path, label, kind, hint }` (`kind: "bool" | "text" | ...`).
- `src/test/{apiMock,render,fixtures,axe,setup}.ts*` **(existing)**: `route("GET /api/v1/...", handler)`, `calls`,
  `failure(status, code, message)`, `resetApi()`, `renderAt(url, path, element)`; `bodySerializer` is passed to
  the handler in `init`. `setup.ts` runs axe over whatever a UI test leaves on screen (`skipAxe()` only for a
  deliberate fragment) and clears `localStorage`.
- `src/dev/` **(existing)**: `/dev/design` demos; every kit component and status gets one.
- `scripts/screens.mjs:35` **(existing)**: the `PAGES` list of `[name, path]`, run by hand against `dev_hub` and
  `npm run dev`, not in CI. `scripts/size.mjs` **(existing)**: the bundle budget (initial JS, 120 kB chunk, CSS).
- `eslint.config.js:42` **(existing)**: `max-lines` 250 counting neither blank lines nor comments.

### 3.2 Introduced by earlier prompts (verify every name in code)

- **25** (`web/src/features/capabilities/`): `CapabilitiesPage.tsx`, `CapabilityList.tsx`, `capabilityText.ts`,
  `capabilities.module.css`, `capabilities.test.tsx`; `web/src/api/hooks/capabilities.ts` (`useCapabilities(name)`);
  `keys.capabilities(name)`; `CAPABILITY_STATES` in `design/fleetVocab.ts` and `capabilityStateMeta` in `tone.ts`;
  `GET /api/v1/hosts/{name}/capabilities` returning rows with `contract, title, category, effect, state,
  catalogue, target_kind, execution, root_agent, runner_installed` and the list of `unknown_contracts`. Hub:
  `crates/domovoi-hub/src/capability/{registry,ceiling}.rs` with `HubRegistry`, `parse_ceiling(Option<&str>) ->
  CeilingParse { Absent, Parsed(CeilingSummary), Malformed { reason } }`; `crates/domovoi-hub/src/api/capability.rs`.
- **24** (merged in the checkout, `crates/domovoi-core/src/capability/ceiling.rs`): `CeilingSummary { enabled,
  entries: Vec<CeilingEntry>, hash: Option<String> }`, `CeilingEntry { contract: ContractId, targets:
  CeilingTargets, max_per_hour: u32, max_active: Option<u32>, max_ttl: Option<SignedDuration>, args:
  BTreeMap<String, i64> }`, `CeilingTargets { Any, Names(BTreeSet<TargetName>) }` (serde `{"kind":"any"}` or
  `{"kind":"names","names":[..]}`).
- **42** (hub, `crates/domovoi-hub/src/api/{packages,packages_jobs,packages_hosts,dto_packages}.rs`): the
  endpoints of section 6.2 below, `packages.enabled` and `packages.approval_window` web settings, the new error
  codes `packages_disabled, package_conflict, package_in_use, store_full, unknown_key, revoked_key,
  bad_signature, digest_mismatch, signature_file_invalid, not_zstd, too_large, plan_changed, host_busy,
  no_such_package, no_such_job`, the failure codes (section 6.5), job status `running | succeeded | failed |
  partial`, target status `queued | transferring | dispatched | succeeded | failed | not_run | outcome_unknown |
  cancelled`.
- **41**: `PackageStatus { active, disabled_untrusted_key, missing_files }`, `KeyState { trusted, unknown, revoked }`,
  `PrivilegeBoundary { in_effect, not_in_effect }`, `PackageFailureCode` values.
- **37** (`src/design/visibility.ts`, `src/components/{visibilityContext,Visible,DetailsScope,Disclosure}.tsx`,
  `src/test/visibility.tsx`): `VisibilityPage`, `Visible`, `DetailsScope`, `visibilityAttrs`, `useVisibility`,
  `VISIBILITY_PAGES`, `expectP0Everywhere`, `renderWithProfile`.
- **30** (`scripts/smoke.mjs`, `scripts/smoke/{server,data,checks}.mjs`, `npm run smoke`): a mock hub that **fails the
  run on any `/api` request it does not list**.

### 3.3 Hub facts for the ceiling endpoint

- `crates/domovoi-hub/src/api/mod.rs` **(existing)**: `Authed(Current)`, `routes()` with `.routes(routes!(...))`,
  `ApiDoc` tags. Existing host routes use `{name}` (`/api/v1/hosts/{name}`, `.../skills`, `.../checks`, ...).
  `crates/domovoi-hub/src/api/error.rs` **(existing)**: `ApiError::{NotFound, Unavailable, ...}`.
- Prompt 25 owns where the hub keeps a connected agent's `hello.ceiling_summary` and `hello.ceiling_hash` (a live
  session fact, memory only; nothing is stored). **Find that accessor in step 1.** If `HubRegistry` exposes no
  way to read the parsed ceiling, add the one in section 6.1; never read it from a second source.
- Tests: `crates/domovoi-hub/tests/hub/api_harness.rs` **(existing)** (`Api`, `a.admin()`, `a.get`),
  `tests/hub/capability.rs` **(introduced by 25)**, `tests/gates.rs` **(existing)**,
  `tests/hub/snapshots/hub__api_routes__openapi.snap` **(existing)** reviewed with `cargo insta review`.

## 4. Exact scope

Every web source file stays at or under 250 lines and every non-test Rust file at or under 500. **(new)** means
the file does not exist yet. Split by responsibility rather than exceed.

### 4.1 Hub (`crates/domovoi-hub`): the read-only ceiling endpoint

| File | Change |
|---|---|
| `src/api/capability_ceiling.rs` **(new)** | `get_ceiling` handler (`GET /api/v1/hosts/{name}/ceiling`, tag `hosts`, operation id `get_host_ceiling`) and the DTOs `CeilingDto`, `CeilingEntryDto`, `CeilingTargetsDto` (6.1) |
| `src/api/mod.rs` | `mod capability_ceiling;` and one `routes!(capability_ceiling::get_ceiling)` line |
| `src/capability/ceiling.rs` or `registry.rs` (prompt 25) | **only if** no accessor exists: `HubRegistry::ceiling(&self, host: HostId) -> CeilingReport` (6.1). No other change to 25's files |
| `tests/hub/capability_ceiling.rs` **(new)**, `tests/hub/main.rs` | the HTTP tests of section 12.1 |
| `tests/gates.rs` | extend with `host_ceiling_is_read_only_through_the_hub` (12.1); never weaken |
| `tests/hub/snapshots/hub__api_routes__openapi.snap`, `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | regenerated (`cargo xtask openapi && cd web && npm run gen:api`), the snapshot read with `cargo insta review`; the two generated files never hand-edited |

No change to `domovoi-core`, `domovoi-engine`, `domovoi-store`, `domovoi-proto`, `domovoi-agent`, `domovoi-config`,
`xtask` or any `Cargo.toml`. No migration. No `ServicesDeps` change: a plain read stays in the `api/` handler
(prompt 29's rule); there is no command here.

### 4.2 Web: data layer and words (`src/api/`, `src/design/`)

| File | Change |
|---|---|
| `src/api/hooks/packages.ts` **(new)** | `usePackagesStatus`, `usePackages`, `usePackageDetail`, `useImportPackage`, `useDeletePackage`, `packageForm` (6.3) |
| `src/api/hooks/packageJobs.ts` **(new)** | `usePlanJob`, `useCreateInstallJob`, `useInstallJobs`, `useInstallJob`, `useRetryTarget`, `useCancelTarget`, `JOB_POLL_MS` (6.3) |
| `src/api/hooks/hostPackages.ts` **(new)** | `useHostPackages`, `useRefreshHostPackages` (6.3) |
| `src/api/hooks/ceiling.ts` **(new)** | `useHostCeiling` (6.3) |
| `src/api/queryClient.ts` | add `packagesStatus`, `packages`, `package(name, version)`, `installJobs`, `installJob(id)`, `hostPackages(hostId)`, `hostCeiling(name)` beside the existing keys; do not touch 25's `capabilities` key |
| `src/api/types.ts` | aliases from the generated schemas: `PackagesStatus`, `PackageSummary`, `PackageDetail`, `InstallPlan`, `InstallPlanTarget`, `InstallJob`, `InstallTarget`, `JobPage`, `HostPackages`, `HostPackage`, `HostCeiling`, `CeilingEntry` (names follow what the generated `schema.d.ts` holds; record the real DTO names in the report) |
| `src/design/packageVocab.ts` **(new)** | the seven tables of 6.4 |
| `src/design/packageFailures.ts` **(new)** | `TARGET_FAILURE_CODES` and the words for each failure family (6.5) |
| `src/design/status.ts`, `src/design/tone.ts` | types for the new tables and one lookup each (`installTargetStatusMeta`, `installJobStatusMeta`, `hostPackageStatusMeta`, `keyStateMeta`, `fileStatusMeta`, `privilegeBoundaryMeta`, `ceilingReportingMeta`), each in `VOCABULARIES`. If `tone.ts` would pass 250 counted lines, move the new lookups to `src/design/packageTone.ts` and re-export them from `tone.ts` |
| `src/design/messages.ts` | add the fifteen codes to `HUB_CODES` and `CATALOGUE` (6.5) and export `describeTargetFailure`; `messages.test.ts` gains the codes in its `STATUS` map |
| `src/design/icons.tsx` | add `package: Package`, `upload: Upload` and, unless 43 already added it, `lock: Lock` |

### 4.3 Web: Capability Center, Host limits, Helper placeholders (`src/features/capabilities/`)

| File | Change |
|---|---|
| `CapabilitiesPage.tsx` (prompt 25) | keeps its `PageHeader`; its body moves into `InventoryTab.tsx`; renders `<CapabilityTabs/>` under the header and wraps the page in `<VisibilityPage page="capabilities">` |
| `CapabilityTabs.tsx` **(new)** | `Tabs` with ids `inventory` (the default, no parameter), `packages`, `limits`, `helper`; the choice in `?tab=` (`useSearchParams`, `replace`) |
| `InventoryTab.tsx` **(new)** | prompt 25's host choice and `CapabilityList`, moved verbatim, plus the `Visible` markers of 6.6. Do not copy 25's list code |
| `LimitsTab.tsx`, `LimitsTable.tsx`, `limitsText.ts` **(new)** | the read-only host ceiling view (6.8) |
| `HelperTab.tsx`, `HelperPlaceholder.tsx`, `HelperKeyPage.tsx`, `HelperKeyPlaceholder.tsx` **(new)** | the two placeholders (6.9) |
| `capabilityCenter.module.css` **(new)** | placement only, tokens only |
| `CapabilitiesPage` tests (prompt 25) | stay green unchanged except for an added tab strip, see 12.2 |

### 4.4 Web: Packages and the install wizard (`src/features/packages/`, all **(new)**)

| File | Change |
|---|---|
| `PackagesTab.tsx` | the tab: status banner, import, store list, installed-on-host, recent installs |
| `PackagesStatusNote.tsx`, `OwnerKeys.tsx` | package installation off / on; owner and revoked key lists, read-only |
| `ImportPackage.tsx`, `ImportResult.tsx` | the two-file upload and its result (6.7.1) |
| `PackageTable.tsx`, `PackageDetailDialog.tsx`, `DeletePackageDialog.tsx` | the store list, details (digest, signature file, hosts), delete |
| `InstalledOnHost.tsx`, `InstalledRows.ts` | per-host installed list with "Check host" (6.7.2) |
| `RecentInstalls.tsx` | the ten newest jobs |
| `InstallWizardPage.tsx`, `wizardState.ts`, `WizardChoose.tsx`, `WizardHosts.tsx`, `WizardReview.tsx`, `PlanCode.tsx`, `AuthorizeDialog.tsx` | the wizard (6.7.3) |
| `InstallJobPage.tsx`, `JobTargets.tsx`, `TargetRow.tsx`, `jobModel.ts`, `RetryDialog.tsx` | job progress, retry, cancel, check host (6.7.4) |
| `packageText.ts` | owner-facing sentences (6.10) |
| `packages.module.css` | placement only, tokens only |

### 4.5 Web: routes, settings, visibility, demos, smoke, docs

| File | Change |
|---|---|
| `src/app/pages.ts`, `src/app/routes.tsx` | lazy pages `packageInstall`, `installJob`, `helperKey`; routes `/capabilities/install`, `/capabilities/jobs/:id`, `/capabilities/helper-key` inside `<Layout/>`, next to 25's `/capabilities` |
| `src/app/PageTitle.tsx` | `defaultDetail`: section `capabilities` with rest `install` gives "Install package", rest starting `jobs` gives "Install job", rest `helper-key` gives "Helper key" |
| `src/features/settings/sections/` | one new section `packages` (new file `packages.ts`, registered in the section list like `upkeep.ts`) with fields `packages.enabled` (`bool`) and `packages.approval_window` (`text`, hint "How long a confirmed install waits for its host to be ready. 1 minute to 1 hour, for example 10m.") |
| `src/design/visibility.ts` (prompt 37) | append `{ key: "capabilities", label: "Capability Center" }` to `VISIBILITY_PAGES` |
| `src/dev/CapabilityDemos.tsx`, `DesignPage.tsx`, `src/dev/capabilityDemos.test.tsx` **(new)** | catalogue demos (12.6) |
| `scripts/smoke/{server,data,checks}.mjs` (prompt 30) | mock routes and four new checks (12.7) |
| `scripts/screens.mjs` | five new `[name, path]` entries (12.7) |
| `DESIGN.md`, `../AGENTS.md`, `../docs/book/...` | section 15 |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Any helper, skill-runner, attestation, key-enrollment or management-path **data, endpoint, hook, DTO or fake** | the helper prompt (H1, gated on R-3, R-9, R-10); the placeholders only reserve its mount points (6.9) |
| Any way to add, remove, rotate or edit a package key, an owner key, a revoked list, the ceiling, `helper.toml` or `agent.toml` | never from the hub (D-03, D-16); file edits on the host |
| Automation of any kind for packages: auto-install, scheduled or recurring installs, "update available" auto-apply, install by policy, install by chat or by the model, remembered approval, "install on all hosts" | never (RW-26, INV-1) |
| Enabling a package's skills, a skill floor or enable switch for package skills | a later prompt; the `/skills` switch is unchanged |
| A network package index, first-party registry fetching, an index URL | not in v1 (D-16); 42 imports only |
| Editing package or install-job data on the hub beyond import, delete, create, retry, cancel and "check host" | not offered |
| Any change to `crates/domovoi-agent`, `domovoi-proto`, `domovoi-store`, the install worker, the package store, step-up rules or arch rules | 41, 42 |
| Operations integration: showing install jobs inside the Operations list or the operation inspector | not planned: a job is its own aggregate, not an operation |
| Policy, knowledge and audit pages; the Control Room; global search | 43, 48, 50, 51 |
| SSE for install jobs (a new topic) | would be a hub and wire-adjacent change; the page polls |
| A graph or playback of installs | no |
| New kit components, new tokens, new motion | none needed; stop and report if one seems needed |
| Seeding packages or jobs into `crates/domovoi-hub/examples/dev_hub.rs` | Rust; `screens.mjs` shows the "off" and empty states, the smoke test shows populated ones |
| Changing prompt 25's `Availability`, `/skills` switch or its tests, prompt 30's pages or `ApprovalCard`, prompt 37's table | out of scope |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly) or **convention** (follow repository style). Every public Rust item gets a
doc comment saying what it is for; every web component a JSDoc saying what it is for and not for. Never write a
prompt number, "the build plan", an RW or D id into code, comments, the book, UI text, commit messages or the PR.

### 6.1 Hub: the read-only ceiling endpoint **fixed** (paths, fields, statuses), **convention** (layout)

```rust
// crates/domovoi-hub/src/capability/ (prompt 25's module): add only if no accessor exists
/// What the hub knows about one host's unattended ceiling right now. Memory only: nothing is stored.
pub struct CeilingReport { pub connected: bool, pub state: CeilingState }
pub enum CeilingState {
    /// No summary reached the hub: the agent reported none, predates it, or is not connected.
    NotReported,
    /// A valid summary, exactly as the agent sent it (prompt 24's `CeilingSummary`).
    Reported(CeilingSummary),
    /// A summary arrived and `parse_ceiling` refused it. `reason` is at most 200 characters, never the raw JSON.
    Unreadable(String),
}
impl HubRegistry { pub fn ceiling(&self, host: HostId) -> CeilingReport; }   // reads the same input `availability` reads
```

`GET /api/v1/hosts/{name}/ceiling` (needs a session, like every route; answers 200, 401, 404 `not_found` for an
unknown host name). A host with no agent answers 200 `not_reported`, `connected: false`. Never a write verb on
this path, never a body.

```json
{ "host": "nas", "connected": true, "reporting": "reported", "unattended_enabled": true, "hash": "9f86d081884c7d659a2f",
  "entries": [
    { "contract": "container.restart", "targets": { "kind": "names", "names": ["jellyfin", "plex"] },
      "max_per_hour": 3, "max_active": null, "max_ttl_secs": null, "args": {} },
    { "contract": "firewall.block_source", "targets": { "kind": "any" },
      "max_per_hour": 20, "max_active": 50, "max_ttl_secs": 3600, "args": { "min_prefix_len_v4": 24 } } ],
  "unreadable_reason": null }
```

Rules (**fixed**):

1. `reporting` is `reported | not_reported | unreadable` (a serde `snake_case` enum). Only `reported` fills
   `unattended_enabled`, `hash` and `entries`; the other two answer `unattended_enabled: null`, `hash: null`,
   `entries: []`. `unreadable_reason` is set only for `unreadable`.
2. `entries` sorted by `contract`, `names` sorted ascending, `max_ttl_secs` is `SignedDuration::as_secs` or null.
3. `hash` is the agent's opaque `ceiling_hash` as reported. The hub does not interpret it and the endpoint
   does not recompute it.
4. The handler reads through `HubRegistry::ceiling` only. It never reads `agent.toml`, never writes a setting
   or the store, never calls the agent, never touches `SettingsRepo`.
5. The DTOs derive `Serialize, ToSchema`, hold no secret and no path, and carry doc comments.

### 6.2 Package and job API the web consumes **fixed** (introduced by 42, section 6.9; verify against the generated `schema.d.ts`)

All need a session; writes also need CSRF (the client does that). Names of DTOs may differ; the paths, verbs and
fields below are what 42 fixed.

| Method and path | Request | Answer | Notes for the UI |
|---|---|---|---|
| `GET /api/v1/packages/status` | none | `{ enabled, max_artifact_bytes, max_store_bytes, used_store_bytes, artifact_count, owner_keys: [{id, label}], revoked: [id], approval_window_secs }` | works when installation is off |
| `GET /api/v1/packages` | none | list of packages: `name, version, archive_sha256, key_id, signer_label?, archive_size, imported_at, imported_by` (and `file_status` if the DTO has it) | works when off; **no N+1**: never fetch details per row |
| `POST /api/v1/packages/import` | multipart: file fields `archive` and `signature`, once each | `201` package, or `200` with `already_imported: true` | no password; `409 packages_disabled` when off |
| `GET /api/v1/packages/{name}/{version}` | none | package plus `signature_toml`, `hosts: [{host_id, host_name, status, observed_at}]`, `file_status: ready \| missing` | opened in a dialog |
| `DELETE /api/v1/packages/{name}/{version}` | none | `204` | `409 package_in_use`; installed copies are untouched |
| `POST /api/v1/packages/jobs/plan` | `{ kind: "install" \| "uninstall", name, version, host_ids: [uuid] }` (1 to 16 hosts) | `{ kind, name, version, archive_sha256?, key_id?, signer_label?, job_plan_hash, targets: [{ host_id, host_name, plan_hash, availability, installed?, warnings: [string] }] }` | read-only; a POST used as a query |
| `POST /api/v1/packages/jobs` | `{ kind, name, version, host_ids, plan_hash, password, confirm_text }` where `plan_hash` is the job hash | `202` job | step-up; `confirm_text` = package name |
| `GET /api/v1/packages/jobs` | query `host_id?`, `name?`, `status?` (`active` or `finished`), `limit?`, `cursor?` | `{ items: [job], next_cursor }` newest first | |
| `GET /api/v1/packages/jobs/{id}` | none | job: `id, kind, name, version, archive_sha256?, key_id?, requested_by, created_at, status: running\|succeeded\|failed\|partial, job_plan_hash, targets: [target]` | status is derived by the hub |
| `POST /api/v1/packages/jobs/{id}/targets/{host_id}/retry` | `{ plan_hash, password, confirm_text }` | `202` job | only from `failed`, `not_run`, `outcome_unknown`; **which hash the route compares is verified in step 1** |
| `POST /api/v1/packages/jobs/{id}/targets/{host_id}/cancel` | none | job | only `queued`; no password |
| `GET /api/v1/hosts/{name}/packages` | none | `{ host_id, observed_at?, entries: [host package], install_allowed? }` | display only |
| `POST /api/v1/hosts/{name}/packages/refresh` | none | same as above | asks the agent; "not connected" maps through `ApiError` |

Target: `id, host_id, host_name, attempt, status, failure?: {code, message}, outputs_confirmed, queued_at,
started_at?, finished_at?, expires_at, privilege_boundary?`. Host package: `host_id, name, version, key_id,
archive_sha256, installed_at, status, key_state, skills: [string], privilege_boundary, observed_at, observed_by`.
`installed` in a plan target is a host package or null. **A job may list several targets for one host (one per
attempt): group by `host_id` and treat the highest `attempt` as the current one (6.7.4).** If the job DTO
instead returns only the latest attempt, the grouping is a no-op; record which in the report.

### 6.3 Hooks **fixed** (names, request shapes), **convention** (layout)

```ts
// src/api/hooks/packages.ts
export function usePackagesStatus();          // GET /api/v1/packages/status; key keys.packagesStatus; staleTime 30_000
export function usePackages();                // GET /api/v1/packages; key keys.packages
export function usePackageDetail(name: string, version: string, enabled: boolean);   // key keys.package(name, version)
export function packageForm(input: { archive: File; signature: File }): FormData;    // fields "archive" and "signature", each with the file's own name
export function useImportPackage();           // POST /api/v1/packages/import, bodySerializer: () => packageForm(input) with a placeholder
                                              // body as notes.ts does; onSuccess invalidates keys.packages and keys.packagesStatus
export function useDeletePackage();           // DELETE; onSuccess invalidates the same two keys

// src/api/hooks/packageJobs.ts
export const JOB_POLL_MS = 3000;
export interface PlanRequest { kind: "install" | "uninstall"; name: string; version: string; hostIds: string[] }
export function usePlanJob(request: PlanRequest | undefined);   // useQuery over POST /packages/jobs/plan; disabled while undefined;
                                              // key ["packages","plan", kind, name, version, sortedHostIds]; staleTime Infinity;
                                              // refetchOnWindowFocus false (the hash must not move under the person's eyes)
export function useCreateInstallJob();        // POST /packages/jobs; variables { request: PlanRequest; planHash: string; password: string; confirmText: string }
export function useInstallJobs(limit: number);  // GET /packages/jobs
export function useInstallJob(id: string);    // GET /packages/jobs/{id}; refetchInterval: (q) => isActive(q.state.data) ? JOB_POLL_MS : false;
                                              // refetchIntervalInBackground false
export function useRetryTarget(jobId: string);   // variables { hostId; planHash; password; confirmText }
export function useCancelTarget(jobId: string);  // variables { hostId }

// src/api/hooks/hostPackages.ts
export function useHostPackages(hostId: string | undefined);
export function useRefreshHostPackages(hostId: string);   // onSuccess: client.setQueryData(keys.hostPackages(hostId), answer)

// src/api/hooks/ceiling.ts
export function useHostCeiling(name: string | undefined);  // GET /api/v1/hosts/{name}/ceiling; key keys.hostCeiling(name); staleTime 15_000
```

Rules (**fixed**):

- **The password is never kept.** `useCreateInstallJob` and `useRetryTarget` are `useMutation` with `gcTime: 0`;
  their callers call `reset()` as soon as the mutation settles, success or failure, so the variables (which
  hold the password) leave the mutation cache. The password sits only in the dialog's `useState` and is
  cleared when the dialog closes and after every submit. Test `password_is_not_kept_after_submit`.
- **Who may call them.** `useCreateInstallJob` is used only in `AuthorizeDialog.tsx`; `useRetryTarget` only in
  `RetryDialog.tsx`. A gate test enforces it (12.3).
- Every mutation call site is wrapped in `useActionFeedback` (and carries `errorDisplay: "inline"` inside a
  dialog). `isActive(job)` is in `jobModel.ts`: true when `job.status === "running"` or any target status is
  `queued`, `transferring` or `dispatched`; an unknown value is **not** active (it never polls forever).
- No hook sends anything but what its row in 6.2 lists. There is no hook for any other package or helper
  route.

### 6.4 Vocabularies **fixed** (words), wiring **convention** (`src/design/packageVocab.ts`; lookups in `tone.ts` through `lookup()`)

Sentence case, no exclamation marks. Each entry: label, tone, icon, hint. An unknown value is a neutral pill with
the raw value (`lookup`). `tone.test.ts` walks every table through `VOCABULARIES`.

`INSTALL_TARGET_STATUSES`:

| key | label | tone | icon | hint |
|---|---|---|---|---|
| `queued` | Queued | neutral | clock | Approved and waiting for the host's agent to be ready. It starts on its own until the approval window closes, then it is dropped. |
| `transferring` | Sending | progress | loader | The hub is sending the package to the host. |
| `dispatched` | Running | progress | loader | The host's agent is carrying it out. The hub is waiting for its answer. |
| `succeeded` | Succeeded | success | check-circle | The host's agent confirmed it. |
| `failed` | Failed | danger | x-circle | The host's agent refused it or could not finish. Its work is all or nothing, so nothing was left half done. |
| `not_run` | Never ran | neutral | ban | Nothing was started on the host: the approval window closed, the host was not ready or the transfer failed. It is safe to try again. |
| `outcome_unknown` | Result unknown | warning | alert | The hub handed it to the host but never got a trustworthy answer. Check the host to see what is installed before trying again. |
| `cancelled` | Cancelled | neutral | x | You cancelled it before it started. |

`INSTALL_JOB_STATUSES`: `running` Running (progress, loader, "At least one host is still working on it.");
`succeeded` Succeeded (success, check-circle, "Every host finished."); `failed` Failed (danger, x-circle, "No host
finished it. Each host shows why."); `partial` Partly done (warning, alert, "Some hosts finished and some did not.
Each host shows its own result.").

`HOST_PACKAGE_STATUSES`: `active` Installed (success, check-circle, "The host holds verified files for this
package. Installed does not mean enabled."); `disabled_untrusted_key` Signed by an untrusted key (warning, alert,
"The host no longer trusts the key that signed this package, so it refuses to run it. Re-sign it with a trusted key
or remove it."); `missing_files` Files missing (danger, x-circle, "The host's record names this package but its
files are gone. Remove it and install it again.").

`KEY_STATES`: `trusted` Trusted key (success, check-circle, "The host trusts the key that signed this package.");
`unknown` Unknown key (warning, help, "The host does not have this key in its trusted list."); `revoked` Revoked
key (danger, ban, "The host has revoked the key that signed this package.").

`FILE_STATUSES`: `ready` In the store (success, check-circle, "The archive is in the hub's package store.");
`missing` File missing (danger, x-circle, "The archive is gone from the hub's package store, so it cannot be
installed. Import the same package again to repair it.").

`PRIVILEGE_BOUNDARIES`: `in_effect` Isolated (success, check-circle, "The agent reports that the privilege
boundary is in effect on this host."); `not_in_effect` Not isolated (warning, alert, "The agent reports that the
privilege boundary is not in effect on this host, for example because it runs as root, so package code is not kept
apart from it.").

`CEILING_REPORTING` (seventh table, same file, lookup `ceilingReportingMeta`): `reported` Reported by the host
(success, check-circle, "The host sent its unattended limits and they are shown below."); `not_reported` Not
reported (neutral, info, "The host has not sent any unattended limits to this hub."); `unreadable` Unreadable
(warning, alert, "The host sent something the hub could not read, so it is treated as not allowed.").

Availability of the install contract in a plan uses prompt 25's `capabilityStateMeta` and its sentences; this
prompt adds no second availability vocabulary.

### 6.5 Words for failures **fixed** (sentences), **convention** (file layout)

**Hub error codes** added to `HUB_CODES` and `CATALOGUE` in `messages.ts` (status for the test map; `specific`
means the hub's own sentence, which names a host or key, is shown as the detail):

| code | status | reason | specific |
|---|---|---|---|
| `packages_disabled` | 409 | Package installation is turned off on this hub. Turn it on in Settings, Packages, then try again. | no |
| `package_conflict` | 409 | That does not fit what the hub has stored for this package or install. | yes |
| `package_in_use` | 409 | A running install still needs this package. Wait for it to finish, then delete it. | no |
| `store_full` | 409 | The package store is full. Delete a package you no longer need, then import again. | no |
| `unknown_key` | 422 | The package is signed with a key that is not in this hub's owner key list. Add the key to hub.toml, restart the hub and import again. | yes |
| `revoked_key` | 422 | The key that signed this package is on the revoked list, so the hub refuses it. | yes |
| `bad_signature` | 422 | The signature does not match the archive. The file may be damaged, or it was not signed by that key. | no |
| `digest_mismatch` | 422 | The archive does not match the digest in its signature file. Get both files again. | no |
| `signature_file_invalid` | 422 | The signature file is not in the expected format. | no |
| `not_zstd` | 422 | The archive is not a .tar.zst file. | no |
| `too_large` | 413 | The file is larger than this hub accepts. | no |
| `plan_changed` | 409 | What you reviewed has changed since. Review the plan again. | no |
| `host_busy` | 409 | Another install is still running on one of these hosts. Wait for it, then try again. | yes |
| `no_such_package` | 404 | That package is not in the store. Import it first. | no |
| `no_such_job` | 404 | That install no longer exists. | no |

Code that does not exist in the hub's `ApiError` after 42 merged must not be added: read the real list in step 1
and add exactly those.

**Target failures.** `describeTargetFailure(failure: { code: string; message: string }): DescribedError` in
`messages.ts` (delegating to `packageFailures.ts` so `messages.ts` stays under the line cap): `title` is the
sentence for the code's family below, `detail` is the agent's or hub's `message` with a leading `"<code>: "`
removed (plain text, never rendered as Markdown, never containing a raw code), omitted when empty or equal to the
title. An unknown code gives title "The host reported a problem this page does not know." and the message as detail.
`TARGET_FAILURE_CODES` lists every code below; test `every_known_failure_code_has_words`.

| Codes | Sentence |
|---|---|
| `host_unavailable` | The host was not connected when the hub tried to start. |
| `agent_incompatible` | The host's agent speaks another protocol version. Upgrade the agent, then try again. |
| `not_advertised`, `contract_unknown` | The host's agent does not offer package installation. Turn it on in the host's agent.toml and restart the agent. |
| `artifact_missing` | The package file is missing from the hub's store. Import the same package again. |
| `artifact_corrupt` | The stored package no longer matches its recorded digest, so the hub did not send it. Delete it and import it again. |
| `transfer_failed`, `transfer_unknown`, `transfer_expired`, `transfer_kind_mismatch` | The package did not reach the host intact. Try again. |
| `session_changed`, `handoff_failed`, `hub_restarted` | The install stopped before the host did anything: the connection changed, the hub could not hand it over or the hub restarted. Nothing ran. Try again. |
| `approval_expired`, `authorization_expired` | The approval window closed before the host was ready. Nothing ran. Start the install again. |
| `result_mismatch` | The host answered, but its answer did not match what you approved. Check the host before trying again. |
| `agent_refused`, `allow_risk_denied` | The host's agent refused the request under its own rules. |
| `rate_limited` | The host's agent is limiting how many changes it accepts. Try again later. |
| `journal_corrupt` | The host's agent cannot record changes right now, so it refused. Check the agent's log. |
| `packages_disabled` | The host has not turned on package installation. |
| `untrusted_key` | The host does not trust the key that signed this package. |
| `revoked_key` | The host has revoked the key that signed this package. |
| `bad_signature`, `signature_file_invalid` | The host could not verify the package signature. |
| `digest_mismatch`, `identity_mismatch` | What the host received does not match what was signed. |
| `too_large`, `limit_entries`, `limit_uncompressed`, `limit_file_size`, `limit_depth`, `limit_path_length`, `limit_time` | The package is larger, deeper or slower to unpack than the host allows. |
| `archive_invalid`, `manifest_invalid`, `incompatible` | The package cannot be used on this host: it is damaged, its manifest is wrong or it needs another version. |
| `path_escape`, `path_invalid`, `link_entry`, `special_entry`, `duplicate_path` | The archive holds an entry the host refuses, such as an unsafe path, a link or a duplicate. Nothing was extracted. |
| `no_skills`, `skills_invalid`, `skill_conflict`, `skill_run_as_forbidden` | The package's skills are not acceptable to this host. |
| `version_conflict` | This version conflicts with one already on the host. |
| `too_many_packages`, `packages_dir_unsafe`, `no_space`, `store_corrupt`, `storage` | The host could not store the package: no room, too many packages, an unsafe directory or a damaged store. |
| `not_run` | Nothing was started on the host. |

### 6.6 Visibility categories of the new screens **fixed** (prompt 37's rule 6.5 applied; a test enforces the P0 ids)

`VisibilityPage page="capabilities"` wraps the Capability Center, the wizard, the job page and the helper-key
page. Mark: **V** = `<Visible>`, **A** = `visibilityAttrs` on a row or cell element, **H** = `useVisibility`.
Page headers, the tab strip, the stepper and section headings are navigation chrome and carry no category (they
are shown in every profile). Dialogs, toasts, menus and tooltips carry none. No P0 element sits inside a P2 or P3
`Visible`; a P3 block sits inside a `DetailsScope`; anything with a decision control is P0.

| id | element | cat | mark |
|---|---|---|---|
| `capabilities.row.state` | the availability pill and its reason on an Inventory row | P0 | A |
| `capabilities.row.authority` | the "runs through the control agent" and "isolated runner not installed" labels | P0 | A |
| `capabilities.row.title` | contract title and effect pill | P1 | A |
| `capabilities.row.technical` | contract id, version, category, execution path, target kind | P2 | V (label "Technical details") |
| `capabilities.catalogue.note` | "catalogue unverified", "host offline" notes | P0 | V |
| `capabilities.unknown` | the list of contracts the hub does not know | P2 | V (label "Unknown contracts") |
| `packages.status.note` | installation off, no owner key | P0 | V |
| `packages.import.form` | the two drop zones and the Import button | P1 | V |
| `packages.import.result` | the import result and every import failure | P0 | V |
| `packages.store.row` | name, version, signer label, Install and Details | P1 | A |
| `packages.store.problem` | "file missing" on a stored package | P0 | A |
| `packages.store.usage` | used space and package count | P2 | V (label "Store usage") |
| `packages.owner-keys` | the owner and revoked key lists | P2 | V (label "Owner keys") |
| `packages.installed.row` | package and version on a host | P1 | A |
| `packages.installed.status` | status pill, privilege-boundary pill, "last checked" | P0 | A |
| `packages.installed.details` | key state, key id, skills | P2 | V (label "Package details") |
| `packages.jobs.row` | recent install: package, hosts, status pill, when | P0 | A (a list of current state and time, like the operations list) |
| `install.choose` | package and kind choice | P1 | V |
| `install.hosts.row` | a host choice | P1 | A |
| `install.hosts.state` | the host's connection state and its note | P0 | A |
| `install.review.summary` | what will be installed or removed, signer, short digest | P0 | V |
| `install.review.host` | per-host availability, installed version, warnings | P0 | A |
| `install.review.plan` | the plan code | P0 | V |
| `install.review.notes` | "not enabled", approval window, signature note, human-only line | P0 | V |
| `install.review.fingerprint` | full digest and key id | P2 | V (label "Package fingerprint") |
| `install.review.confirm` | the "Review and confirm" button and any plan-changed notice | P0 | V |
| `job.status` | the job's status pill | P0 | V |
| `job.target.row` | a host's row | P1 | A |
| `job.target.status` | target status, failure sentence, wait or confirm note, boundary pill | P0 | A |
| `job.target.actions` | Retry, Cancel, Check host | P0 | A (decision controls) |
| `job.target.history` | earlier attempts of a host | P2 | V (label "Earlier attempts", count) |
| `job.details` | plan code, approval expiry, requested by | P3 | V in a `DetailsScope` |
| `limits.note` | "these limits belong to the host" | P0 | V |
| `limits.status` | reported, not reported, unreadable, offline, and what each means | P0 | V |
| `limits.table` | the entries | P1 | V |
| `limits.hash` | the configuration fingerprint | P3 | V in a `DetailsScope` |
| `capabilities.helper.placeholder` | the helper placeholder | P0 | V |
| `capabilities.helper-key.placeholder` | the key-enrollment placeholder | P0 | V |

Add `capabilities` to `VISIBILITY_PAGES`. A test (`p0_visible_in_every_profile`, 12.5) renders the Inventory tab,
the Packages tab, the Host limits tab, the Helper tab, the wizard's review step and the job page in all three
profiles and asserts every P0 id above that the fixture produces is present and not collapsed.

### 6.7 Screen behaviour **fixed** (behaviour), **convention** (markup)

Every screen has loading, empty, error and offline states in words. A failed request is a `LoadError`, never an
empty state. Hub and agent texts are plain text, never Markdown.

#### 6.7.1 Packages tab: status, import, store

- Tab body order: `AllHostsNote` ("The package store belongs to the hub, so this part ignores the host selector.";
  the Installed section below does follow the selector), `PackagesStatusNote`, Import, Package store, Installed on
  a host, Recent installs.
- **`PackagesStatusNote`.** Installation off (`enabled: false`): `InlineMessage` tone `info`, title "Package
  installation is off", body "This hub does not accept or install packages yet. Turn it on in Settings, Packages
  (it asks for your password), then import a package." plus a `LinkButton` to `/settings/packages`. It is not an
  error and not red. On with no owner key: tone `warning`, "No owner key is set, so no package can be imported.
  Add a `[[packages.owner_keys]]` entry to hub.toml and restart the hub." (plain text, no Markdown backticks).
- **Import.** Two `DropZone`s, `multiple={false}`: "Package archive" (`accept=".tar.zst"`, hint "The
  name-version.tar.zst file.") and "Signature file" (`accept=".sig.toml"`, hint "The matching
  name-version.sig.toml file."). The chosen file's name and size show with a `Remove` `IconButton`. Button
  "Import package": `disabledReason` "Choose the archive and its signature file." until both exist, and "Package
  installation is off on this hub. Turn it on in Settings, Packages." while off. If the two file stems differ,
  an `InlineMessage` `warning` says "These two files may not belong together." but does not block: the hub
  decides. Submit goes through `useActionFeedback(useImportPackage(), { error: "import the package",
  errorDisplay: "inline", success: null })`; the page shows the result itself.
- **Import result** (`ImportResult`, P0): `InlineMessage` tone `success`, title "Imported {name} {version}" (title
  "{name} {version} is already in the store" for `already_imported`), with a `KeyValueList`: Signed by (the signer
  label, or "a key with no label", plus the key id in mono), Digest (first 12 hex, full value in a `Tooltip`),
  and the `SIGNATURE_NOTE` sentence (6.10). It offers a `LinkButton` "Install it" to
  `/capabilities/install?package={name}&version={version}`. The two chosen files clear after a success.
  A failure shows inside the same block through `describeError` (the codes of 6.5).
- **Store list.** `DataTable` (stable `columns`), caption "Imported packages": Package (name, mono version),
  Signed by (label or key id), actions: `Install…` (`LinkButton` to the wizard; when installation is off or the
  file is missing show a `Button` with `disabledReason` instead) and `Details`. Empty: `EmptyState` "No packages
  yet" with "Import an owner-signed package above." Loading `Skeleton`, error `LoadError` with Retry.
- **Details dialog.** `usePackageDetail` when opened: `KeyValueList` (Name, Version, Signed by, Imported
  `RelativeTime`, Size, `file_status` pill from `FILE_STATUSES`), Digest and Signature file in `CopyBlock`s (the
  signature file is public data, not a secret), and "Installed on" listing host names with `HOST_PACKAGE_STATUSES`
  pills and "as last reported {RelativeTime}". A danger `Button` "Delete from the store" opens
  `DeletePackageDialog`: `ConfirmDialog` tone danger, title "Delete {name} {version} from the store?", consequence
  `DELETE_CONSEQUENCE`, `errorDisplay: "inline"` (a `409 package_in_use` reads in words inside the dialog).
- **Owner keys** (P2): the hub's `owner_keys` as label and key id, and `revoked` ids, read-only, with "These keys
  come from hub.toml. This hub uses them only to refuse files nobody could install. Each host decides for itself
  which keys it trusts, and the hub cannot change them." No control.

#### 6.7.2 Packages tab: installed on a host, recent installs

- **Installed on a host** follows the host selector. Under "All hosts": a `Text muted` line "Choose a host to see
  what is installed on it." and the same host chooser prompt 25's Inventory tab uses (reuse it; if it takes no
  prompt text, add one optional prop rather than copying the component; its links keep `?tab=packages`).
- With a host: `useHostPackages(hostId)`. Header line: "What {host} last reported" and `Last checked
  {RelativeTime}` or "Not checked yet"; `Button` "Check host" (icon `refresh`, `hint` "Asks the host's agent what is
  installed now.") through `useActionFeedback(useRefreshHostPackages(id), { success: "Checked {host}", error:
  "check the host" })`; a host that is not connected reads through `describeError` (`unavailable`). `DataTable`:
  Package, Status (`HOST_PACKAGE_STATUSES` pill, P0), Key (key id and `KEY_STATES` pill, P2 details), Boundary
  (`PRIVILEGE_BOUNDARIES` pill, P0), and a `Remove…` `LinkButton` to the wizard with `kind=uninstall&package=&version=&host=`.
  The note "Installed does not mean enabled." is shown once under the table. If `install_allowed` is `false`:
  `InlineMessage` info "{host}'s agent has not turned on package installation, so an install there would not run."
  Empty: `EmptyState` "Nothing installed" with "Packages you install on {host} appear here."
- **Recent installs**: `useInstallJobs(10)`: rows Package, Hosts (names, "+N"), Status pill (`INSTALL_JOB_STATUSES`),
  Started (`RelativeTime`), whole row links to `/capabilities/jobs/{id}`. Empty: `EmptyState` "No installs yet".

#### 6.7.3 The wizard (`/capabilities/install`)

URL parameters (all optional): `kind` (`install` default, or `uninstall`), `package`, `version`, `host` (a host name).
The selected host in the selector preselects that host if `host` is absent. Initial step: `hosts` when `package` and
`version` are present, else `choose`. **Opening the page with parameters never sends anything**: only the plan read
(on the review step) and the confirm button's request exist.

```ts
// wizardState.ts (pure, unit-tested)
export type Step = "choose" | "hosts" | "review";
export interface WizardState { step: Step; kind: "install" | "uninstall"; name: string; version: string; hostIds: string[] }
export function initialState(params: URLSearchParams, hosts: Host[], selected: string | undefined): WizardState;
export function canContinue(state: WizardState): { ok: true } | { ok: false; reason: string };  // the reason feeds `disabledReason`
export const MAX_HOSTS = 16;
```

`Stepper` (label "Install steps") with steps Package, Hosts, Review, Confirm, Progress: earlier steps `done`, the
current one `current`, Confirm is `current` while the dialog is open, Progress is always `upcoming` here.

1. **Choose** (`WizardChoose`, P1). `SegmentedControl` Install or Remove. Install: a `Select` of the store's packages
   ("{name} {version}"); a package whose file is missing is listed but flagged and blocks Next with "The package file
   is missing from the store. Import it again." Remove: shows the package from the URL read-only, or a `Select` of
   the store's packages, with "To remove a package that is not in the store, open the host's installed list and
   choose Remove there." Next: `disabledReason` "Choose a package first."
2. **Hosts** (`WizardHosts`). One `Checkbox` per host from `useHosts()` with its connection `StatusPill` (P0
   `install.hosts.state`) and, for `offline` or `never_connected`, the note "Not connected now. If you go on, the
   install waits up to {N} minutes for it." ({N} from `approval_window_secs`); `incompatible` and
   `expected_incompatible`: "Needs an agent upgrade first. It will be skipped."; a host with no agent (`hub_only`)
   is shown disabled with "No agent on this host." Counter "{n} of up to 16". Next: `disabledReason` "Choose at
   least one host." / "Choose at most 16 hosts."
3. **Review** (`WizardReview`). Calls `usePlanJob`; `QueryBoundary` with `context: "work out the plan"`. Shows, in
   this order: the summary card (`install.review.summary`); one row per host (`install.review.host`: host name,
   availability `StatusPill` through prompt 25's `capabilityStateMeta` with its sentence, "Installed now: {version}
   {status}" or "Not installed", warnings as `InlineMessage warning`); the plan code (`PlanCode`); the notes
   (`install.review.notes`: `INSTALL_NOT_ENABLE`, `APPROVAL_WINDOW`, `SIGNATURE_NOTE`, `HUMAN_ONLY`); the full
   fingerprint (P2). `Back`, and the primary button "Review and confirm" which opens `AuthorizeDialog`; its
   `disabledReason` is "None of these hosts can run this now. Go back to choose others." when no target's
   availability is `available`, `host_unknown` or `host_absent`. A `409 plan_changed` from the dialog closes it,
   refetches the plan and shows `InlineMessage` warning "The plan changed. Review it again." above the review.
4. **Confirm** (`AuthorizeDialog`, a `ConfirmDialog`): title "Install {name} {version} on {n} host(s)?" ("Remove ..."
   for uninstall); consequence "{name} {version} is sent to {hosts} and installed there. It is not enabled. This
   approval is bound to plan {code}." (uninstall: "... is removed from {hosts}. This approval is bound to plan
   {code}."); `requireText` = the package name; children: a `Field` "Your password" with `PasswordInput`
   (`autoComplete="current-password"`) and the `HUMAN_ONLY` sentence; `confirmLabel` "Install" or "Remove"
   (tone `danger` for Remove, `primary` for Install); `confirmDisabledReason` "Enter your password first." until
   the password is set (the typed-name reason comes from `requireText`); **the full job hash is captured in state
   when the dialog opens**, and if a refetch changes it while the dialog is open the confirm is disabled with "The
   plan changed while you were looking. Close this and review it again." The request body is
   `{ kind, name, version, host_ids, plan_hash: <captured>, password, confirm_text: <typed> }`. On success the
   password is cleared, a toast says "Started installing {name} on {n} hosts" (or "removing"), and the page
   navigates to `/capabilities/jobs/{id}`. Failures stay inside the dialog in words (`403` for a wrong password or
   typed name, `429` with the countdown, `409 host_busy` naming the host, `409 plan_changed`, `409 packages_disabled`).
5. **No automation.** The wizard has no switch, schedule, recurrence, "apply to all hosts", "remember this
   approval", "update automatically" or policy control, and no code in `src/features/packages/` calls
   `useCreateInstallJob` or `useRetryTarget` outside the two dialogs.

#### 6.7.4 The job page (`/capabilities/jobs/:id`)

`useInstallJob(id)` polls every `JOB_POLL_MS` while `isActive` and stops when finished. `AllHostsNote` ("An install can
reach several hosts, so this page shows all of them."). A `404`/`no_such_job` reads "No such install" with a link
back to `/capabilities?tab=packages`.

- Header: title "{Install|Remove} {name} {version}", `PageHeader` meta with the job's `INSTALL_JOB_STATUSES` pill
  (`job.status`), intro "Requested {RelativeTime}. This page refreshes every few seconds while a host is still
  working." Back `LinkButton`.
- **Hosts** (`JobTargets`): targets grouped by `host_id` (`jobModel.ts`: `currentTargets(job)` returns the highest
  `attempt` per host, host name order; `earlierAttempts(job, host)` the rest). `TargetRow` columns: Host (link to
  `/hosts/{name}`), Result, When, Actions.
  - Result: the `INSTALL_TARGET_STATUSES` pill (`job.target.status`), then per status: `queued` "Waiting for {host}.
    It is dropped in {Countdown to expires_at} if the host is not ready."; `transferring`/`dispatched` a `Progress`
    "Sending to {host}…" / "Running on {host}. Waiting for the result…"; `failed`, `not_run`, `outcome_unknown` the
    failure sentence from `describeTargetFailure` (an `InlineMessage` of the target's tone); `succeeded` with
    `outputs_confirmed === false` the line "Confirmed by the host's record only. Check the host to see what is
    installed."; the privilege-boundary pill when `privilege_boundary` is present.
  - Actions (all P0): **Retry** (`failed`, `not_run`, `outcome_unknown`) opens `RetryDialog`; **Cancel** (`queued`
    only) is a `Button` with `hint` "Only an install that has not started can be cancelled." and posts without a
    password, answering through `useActionFeedback` (`success: "Cancelled the install on {host}"`, `error: "cancel
    the install"`, `errorDisplay: "inline"`); **Check host** (`outcome_unknown`, and `succeeded` when not confirmed)
    calls `useRefreshHostPackages` and then says in an `InlineMessage` "{host} reports {name} {version} installed" or
    "{host} does not list {name} {version}" from the answer. No other button exists on a row.
  - **Earlier attempts** (P2, label "Earlier attempts", count): attempt number, status pill, failure sentence.
- **Details** (P3 in a `DetailsScope`): plan code (first 8 hex), approval window expiry, requested by (only when the
  DTO gives a readable name).
- `RetryDialog`: for one host. It calls `usePlanJob({ kind, name, version, hostIds: [hostId] })` inside the dialog
  (`QueryBoundary`, `context: "work out the plan"`), shows the host, the availability sentence, the current failure
  sentence and the plan code, then asks for the password and the typed package name exactly like
  `AuthorizeDialog`, with the same capture rule. The submitted `plan_hash` is the value the retry route compares,
  **verified in step 1**: expected to be that host's `plan_hash` from the fresh single-host plan; if the code
  compares the job hash instead, send `job_plan_hash` and record it in the report. On success: toast "Started
  again on {host}", the dialog closes, the job query is invalidated (polling resumes).

### 6.8 Host limits tab **fixed** (behaviour)

`LimitsTab` follows the host selector (under "All hosts": the chooser, as in 6.7.2, links keep `?tab=limits`). With a
host, `useHostCeiling(name)` inside `QueryBoundary` (`context: "load the host limits"`). **The tab has no input, no
switch, no save, no mutation and no write request**: it reads and displays. Test `limits_tab_is_read_only`.

1. `limits.note` (P0) `InlineMessage` info, title "These limits belong to the host", body `CEILING_READ_ONLY` (6.10).
2. `limits.status` (P0), by the answer: `reported` and `unattended_enabled` → a `reported` pill and "Unattended actions
   are allowed within the limits below."; `reported` and not enabled → "Unattended execution is off on this host.
   Nothing runs without your approval."; `not_reported` and `connected` → `CEILING_NOT_REPORTED`; `not_reported` and
   not connected → "{host} is offline, so it has not reported its limits to this hub. They arrive when its agent
   connects."; `unreadable` → `InlineMessage` warning "The hub could not read what {host} reported." with the
   reason as detail and "Until it can, the hub treats unattended actions on this host as not allowed."; an unknown
   `reporting` value → a neutral pill through `lookup` and the sentence "This page does not know how to read that."
3. `limits.table` (P1) when reported and `entries` is not empty: `DataTable`, caption "Unattended limits reported by
   {host}", columns Capability (contract id, mono), Targets (`Any target` as a `Tag`, or each name as a `Tag`, at most
   6 then "+N more" with a `Tooltip` listing the rest), Per hour (numeric), Active at once (numeric, "–" with a
   `Tooltip` "This entry sets no limit; the host's other limits still apply." when null), Longest hold (`1 h`,
   `30 min`, or "–"), Argument bounds (`key: value` pairs in mono, "–" when empty). Reported and enabled with no
   entries: "{host} reports no capability that may run unattended, so none does."
4. `limits.hash` (P3, `DetailsScope`): "Configuration fingerprint" with the first 12 characters, the full value in a
   `Tooltip`, and the hint "Lets Domovoi notice that the host's limits changed. It is not a check that they are right."
5. Loading `Skeleton`; error `LoadError` with Retry; a host that disappears reads "No such host" with a link back.
   The number formatting helpers (`formatHold(secs)`, `targetsText`) live in `limitsText.ts` and are unit-tested.

### 6.9 Helper views: placeholders now, and exactly what the helper prompt must add **fixed**

**What this prompt builds** (and nothing more):

- `HelperTab` (the `helper` tab body, per host or not: it is host-independent because there is nothing to show) renders
  `HelperPlaceholder`. `HelperKeyPage` at `/capabilities/helper-key` (not in the navigation; reached from a link in the
  Helper tab) renders `PageHeader` title "Helper key", intro "Pin the helper's public key after comparing fingerprints.",
  and `HelperKeyPlaceholder`, with a `LinkButton` back to `/capabilities?tab=helper`.
- Both placeholders are one `EmptyState` (icon `lock`) with a `Tag` "Not available yet", the title **"Not available on
  this hub yet"** and these bodies (**fixed** words):
  - Helper: "This hub does not collect helper status, safety coverage or helper keys yet, so there is nothing to show
    here and no data is made up. This is a statement about the hub, not about your hosts."
  - Key enrollment: "Pinning a helper key means comparing the fingerprint the hub received with the one printed on
    the host. This hub does not receive helper keys yet, so there is nothing to compare or confirm."
- They are `Visible` P0 with ids `capabilities.helper.placeholder` and `capabilities.helper-key.placeholder`.
- **No request, no hook, no DTO, no fake data, no mock.** The components import nothing from `src/api/`. The
  smoke mock lists no helper route. A gate test (12.3) fails if any file in `src/api/` or in
  `src/features/capabilities/Helper*` mentions a helper, attestation or enrollment route; its failure text says
  "when the hub gains helper endpoints, replace the placeholder and update this test".
- Words that never appear on these screens: "available" (except in "Not available"), "enrolled", "pinned",
  "verified", "covered", "attested", "ready", "healthy". Nothing may suggest a host has or lacks a helper.
- The existing `root_agent` and `runner_installed` labels of the Inventory tab (prompt 25) are the only helper-related
  statements in the Capability Center.

**What the helper prompt (H1) must add later**, none of which this prompt invents (names are theirs to choose):

1. **Hub backend**, behind the use-case layer: a per-host read of `helper_status` (`absent`, `error{reason}`,
   `ok{ops}`), `skill_runner_status`, `runs_as_root` and the helper's public key as the hub-computed fingerprint
   (`hello` and `agent_status`, wire 1.2, already on the wire); a store (migration) and audit rows for **pinning,
   changing and unpinning** a helper key per host; commands to pin a key after the owner **types or confirms the
   fingerprint** and to unpin (step-up: password, typed confirmation); the verified **attestation** view
   (effective safety set from the signed report, its freshness, `boot_id`, `generation`, `config_hash`,
   `route_state_hash`, the fail-closed reasons `state_missing`, `state_corrupt`, `state_key_mismatch`, `key_missing`,
   `uninitialized`, `generation_regressed`, and the refusal reason `safety_changed`); the covered and uncovered
   management paths judged only against verified data, and the confirmation of covered paths only; DTOs and
   OpenAPI for all of it; a live-update topic if wanted.
2. **Web**: replace the body of `HelperPlaceholder` (and keep the tab id `helper`) with the helper view: status per
   host, the verified effective set read-only beside the confirmed management paths (each marked covered or
   uncovered), attestation freshness, an "unverified" state when no fresh verified report exists, and the
   instruction to edit `helper.toml` on the host and re-check; replace `HelperKeyPlaceholder` with the enrollment
   flow (the received key's full fingerprint shown as **unverified** until the owner confirms it against the host's
   own printout; "helper key changed: re-enroll" blocking containment; the fail-closed reasons with their local
   action); new statuses (root-agent warning, skill runner unavailable, helper unavailable, key states, attestation
   freshness) in the `design/` vocabularies; the unverified-human variant of the approval card for containment; the
   mock routes, the smoke checks and `screens.mjs` pages; update the `VISIBILITY` ids (the two `placeholder` ids
   disappear); **delete the placeholder gate test and replace it with real ones** (do not weaken it silently).
3. **Docs**: the helper pages of the book, `web/DESIGN.md` (remove "Planned" text that then exists).

### 6.10 Fixed sentences (`packageText.ts`, `limitsText.ts`)

```ts
export const INSTALL_NOT_ENABLE = "Installing puts the package's files on the host. It does not enable anything.";
export const SIGNATURE_NOTE =
  "This hub checked the signature against its owner key list. Each host checks it again against the keys in its own configuration, and refuses a package it does not trust. The hub cannot add or change those keys.";
export const HUMAN_ONLY = "Every install and removal needs your password and the package name typed by you, each time. Nothing installs on its own.";
export const approvalWindow = (n: string) => `After you confirm, each host has ${n} to be ready. A host that is not ready in time is skipped and nothing runs on it.`;
export const DELETE_CONSEQUENCE = "Hosts that already have it keep it. You can import it again later.";
// limitsText.ts
export const CEILING_READ_ONLY =
  "The host decides what may run unattended, and Domovoi shows what the host reported. It cannot change these limits. To change them, edit the [unattended] section of /etc/domovoi/agent.toml on the host and restart the agent.";
export const ceilingNotReported = (host: string) =>
  `${host} has not reported any unattended limits. Until it does, Domovoi treats unattended actions on it as not allowed, and anything that changes the host needs your approval.`;
```

A test (`copy_never_claims_enabled_or_available_early`, 12.4) renders every screen and fails on any of these
phrases: "ready to use", "now running", "runs automatically", "automatic", "auto-install", "update all", "install on
all", "everything is allowed", "no limits", "trusted by your hosts", and on "available" next to "helper" outside
"Not available".

## 7. Ordered implementation steps

Each step is small enough to finish and test on its own. Do not start a step whose predecessor is red.

1. **Inspect and verify the repository context; report discrepancies.** Open every file and symbol of section 3.
   Confirm 25, 30, 37, 41 and 42 are merged and read their real names. Specifically check and write down:
   (a) the real DTO names and fields in the generated `web/src/api/schema.d.ts` for every row of 6.2 (and whether the
   job DTO lists one target per attempt or only the latest, and whether `PackageDto` carries `file_status`);
   (b) the **real route parameter names** 42 used (`{host_id}` or `{name}`) and whether they coexist with `{name}`
   routes; use whatever the code has;
   (c) which hash the retry route compares (per-host plan hash or job hash) in `crates/domovoi-hub/src/api/packages_jobs.rs`;
   (d) the real list of error codes in `ApiError` (6.5 may need fewer or more rows);
   (e) how prompt 25 holds a connected agent's ceiling summary and whether `HubRegistry` already has an accessor;
   (f) what 25's `CapabilitiesPage`, `CapabilityList`, host chooser and tests look like, and that `useCapabilities` exists;
   (g) the real names in 37 (`Visible`, `DetailsScope`, `visibilityAttrs`, `VisibilityPage`, `VISIBILITY_PAGES`,
   `expectP0Everywhere`) and in 30's smoke files; (h) that `packages.enabled` and `packages.approval_window` exist
   as settings; (i) `ICONS` for `package`, `upload`, `lock`; (j) that **no** helper, attestation or enrollment route
   exists in the generated schema (if one does, stop and report: the placeholders may be obsolete). If any
   difference changes what you would build, stop and report.
2. **Hub: the ceiling endpoint** (6.1): the accessor if needed, `capability_ceiling.rs`, the route, the tests and the
   gate of 12.1; then `cargo xtask openapi && cd web && npm run gen:api`, `cargo insta review` (read the snapshot).
   Run the hub checks.
3. **Web foundations**: `types.ts` aliases, `queryClient.ts` keys, the four hook files (6.3), the seven vocabulary
   tables, lookups and `VOCABULARIES` entries, `packageFailures.ts` and `describeTargetFailure`, the fifteen codes in
   `messages.ts` and its test map, the icons. `npm test -- design` and `npm run typecheck`.
4. **Capability Center shell** (4.3): move 25's body into `InventoryTab.tsx` (no copy), add `CapabilityTabs`, the
   `VisibilityPage` wrapper and the Inventory markers of 6.6; 25's tests must still pass.
5. **Host limits tab** (6.8) with `limitsText.ts` and its tests.
6. **Helper placeholders** (6.9): the tab, the page, the route, the gate test.
7. **Packages tab, import and store** (6.7.1), then **installed and recent** (6.7.2), with tests.
8. **Wizard** (6.7.3): `wizardState.ts` first (pure, tested), then the steps, `PlanCode`, `AuthorizeDialog`.
9. **Job page** (6.7.4): `jobModel.ts` (pure, tested), `JobTargets`, `TargetRow`, `RetryDialog`.
10. **Settings section "Packages"** and the routes, lazy pages and page titles (4.5).
11. **Visibility**: add the page key, finish all markers, the `p0_visible_in_every_profile` cases.
12. **Dev catalogue, smoke mock and checks, `screens.mjs`** (12.6, 12.7).
13. **Docs** (section 15).
14. **Checks and the completion report** (the last section): `cd web && npm run format:check && npm run lint &&
    npm run typecheck && npm test && npm run build && npm run smoke`; `cargo fmt --all`, `cargo clippy -p domovoi-hub
    --all-targets -- -D warnings`, `cargo nextest run -p domovoi-hub`, `cargo xtask arch-check`,
    `cargo xtask docs-links`; and `node scripts/screens.mjs` by hand if a dev hub is available (say if it was not run).

## 8. Database and persistence

Not applicable: no migration, no table, no stored value. The ceiling answer is built from the live session's
memory. The package store, install jobs and host inventory are prompt 42's and are only read and called through
HTTP. The only browser storage touched is prompt 37's existing display-preference cache; nothing from this prompt
is persisted in the browser (the wizard's state is in memory and in the URL; **no password, hash or token is stored**).

## 9. Protocol and compatibility

Not applicable to the wire: no message, field or version changes (1.2 is frozen). HTTP only: one new `GET`
(6.1) and calls to prompt 42's endpoints. An older hub without the packages endpoints answers 404 and every
read in the Packages tab shows a `LoadError`; the embedded UI and the hub ship together, so this only affects a dev
server pointed at an old hub. A hub that is newer than the UI may report a status value this UI does not know:
every vocabulary lookup shows it as a neutral pill with the raw text, and polling treats an unknown job or target
status as not active.

## 10. Error handling and recovery

| Situation | Behaviour |
|---|---|
| Any load fails (status, store list, detail, plan, jobs, host packages, ceiling) | `LoadError` with Retry in place of the content, in words from `describeError`; never an empty state |
| `GET /packages/status` fails | the Packages tab shows `LoadError`; import and the wizard's Next stay disabled with "Could not check whether package installation is on." as `disabledReason` |
| Installation off (`enabled: false`) | status note (info); import disabled with a reason; Install buttons disabled with a reason; the list and installed views still load |
| Import: any hub refusal | inside the import block in words (6.5); the chosen files stay so the person can fix and retry; success clears them |
| Import: both files chosen but names suggest different packages | warning only; the hub decides |
| Plan fails (404 no such package, 409 packages_disabled) | `LoadError` in the review step with Retry and a `Back` button |
| Plan shows a host that cannot run it | per-host warning and availability pill; "Review and confirm" blocked only when no host can run it |
| Confirm: wrong password or typed name (403) | inside the dialog, in words; the dialog stays open; the password field is cleared |
| Confirm: too many attempts (429) | inside the dialog with the retry countdown; confirm disabled until it ends |
| Confirm: `plan_changed` (409) | dialog closes, plan refetched, "The plan changed. Review it again." above the review; nothing was created |
| Confirm: `host_busy` (409) | inside the dialog, naming the host; nothing was created |
| Confirm succeeds but the page navigates and the job read fails | the job page shows `LoadError` with Retry; the job exists on the hub (the toast already said it started) |
| Job poll fails while data is on screen | the data stays; a muted line "Could not refresh. Trying again." replaces nothing else; polling continues at the normal interval |
| Target `outcome_unknown` | the warning sentence, "Check host" and "Retry" (a retry is a new approval); the page never says it failed |
| "Check host" while the host is offline | in words through `describeError` (`unavailable`), inline in that row |
| Cancel on a target that has already started (409) | inline in that row in the hub's words; the row refreshes |
| Delete while an install uses the package (409 `package_in_use`) | inside the delete dialog in words |
| Ceiling `unreadable` | warning, reason shown, "treated as not allowed"; never styled as success |
| Ceiling for a host that disconnects while the tab is open | the next read shows "offline … has not reported"; the last rows are not kept as if current |
| An unknown status or reporting value | neutral pill with the raw value and "not known to this version of the UI"; no crash |
| Helper tab or helper-key page | static placeholder; nothing can fail because nothing is requested |

## 11. Security requirements

| Requirement | How it is met | Test |
|---|---|---|
| **Package install and removal are human-only** | the only job-creating and retrying call sites are the confirm buttons of two dialogs, after a typed package name and a password; no effect, timer, URL parameter, chat or policy path calls them; the wizard has no automation control | `job_mutations_have_exactly_two_call_sites`, `wizard_never_posts_before_confirmation`, `wizard_offers_no_automation` |
| **Approval bound to what was shown** | the job hash captured when the dialog opens is the one submitted; a changed hash blocks confirm; the UI never computes or edits a hash | `authorize_sends_the_hash_captured_when_the_dialog_opened`, `plan_change_under_an_open_dialog_blocks_confirm` |
| **Password never kept** | held only in dialog state, cleared on close and after each submit; mutations use `gcTime: 0` and `reset()`; never in a URL, query key, log, storage or toast | `password_is_not_kept_after_submit` |
| **No key authority** | the owner and revoked key lists are read-only text; no control, hook or request can add, change or remove a key anywhere | `key_lists_are_read_only_text`, source scan in `capability_center_has_no_authority_controls` |
| **Signature wording is honest** | "checked by this hub"; "each host checks it again"; never "trusted by your hosts" | `copy_never_claims_enabled_or_available_early` |
| **Installed is not enabled** | no screen says enabled, ready or running about a package | same test |
| **Ceiling is read-only** | `GET` only; no write request in the tab; hub handler has no write code | `limits_tab_is_read_only`, `host_ceiling_is_read_only_through_the_hub` |
| **Unreadable or absent ceiling is never permissive** | words say "treated as not allowed"; never "no limits" or "everything is allowed" | `ceiling_unreadable_is_not_permissive`, copy test |
| **No invented helper data** | the helper views request nothing and show nothing but the placeholder text | `helper_views_are_labelled_placeholders_with_no_requests`, `capability_center_has_no_helper_data_path` |
| **Hub and agent text is data** | failure and warning texts render as plain text; no Markdown, no HTML; a leading code prefix is stripped; raw codes never show | `target_failure_text_is_plain_and_has_no_raw_code` |
| **No secrets or paths** | no hub disk path, no secret value, no signature private data is rendered; the signature file shown is public | `package_screens_show_no_hub_path` |
| **Uploads** | the web sends exactly the two named file fields with the files' own names; extension and size hints are conveniences, the hub is the authority | `import_sends_archive_and_signature_as_multipart` |
| **Profiles are display only** | no approval, confirm, plan or request code reads the visibility profile | gate `visibility_never_gates_package_authority` (12.5) |
| **Fail closed on unknown** | unknown availability, job, target or reporting values render neutral and are never treated as success or as "can run" | `unknown_values_render_neutral_and_never_as_success` |

## 12. Testing requirements

Web tests use `vi.mock("../../api/client", () => import("../../test/apiMock"))`, `route`, `calls`, `failure`,
`resetApi`, `renderAt` (and `renderWithProfile` for profile cases). **Axe runs automatically** over whatever a test
leaves on screen, in the jsdom theme the setup uses; do not call `skipAxe()` except for a deliberate fragment. No
test sleeps: polling tests use `vi.useFakeTimers()` and advance by `JOB_POLL_MS`. Name tests after the behaviour.

### 12.1 Hub (`crates/domovoi-hub/tests/hub/capability_ceiling.rs`, `tests/gates.rs`)

Set the ceiling up exactly as prompt 25's tests do (verify in `tests/hub/capability.rs`); add no second mechanism.

| Test | Setup, action, expected |
|---|---|
| `ceiling_endpoint_reports_not_reported_when_no_agent_sent_one` | connected host, no summary; 200 `reporting: "not_reported"`, `connected: true`, `entries: []`, `hash: null` |
| `ceiling_endpoint_returns_the_summary_as_the_agent_reported_it` | valid summary with one `names` and one `any` entry, `max_ttl`, `args`; fields equal the summary, `max_ttl_secs: 3600` |
| `ceiling_entries_are_sorted_by_contract_and_names_ascending` | unsorted input; sorted output |
| `ceiling_endpoint_marks_a_malformed_summary_unreadable_and_never_echoes_it` | malformed JSON with a marker string; `reporting: "unreadable"`, `unreadable_reason` at most 200 chars and without the marker, entries empty |
| `ceiling_endpoint_for_a_host_with_no_agent_is_not_reported_and_not_connected` | hub-only or disconnected host; 200, `connected: false`, `not_reported` |
| `ceiling_endpoint_requires_a_session` | no session; 401 |
| `ceiling_endpoint_404s_on_an_unknown_host` | `/hosts/nope/ceiling`; 404 `not_found` |
| `ceiling_dto_covers_every_field` | every field of 6.1 present on a reported answer |
| `host_ceiling_is_read_only_through_the_hub` (gate) | scan `crates/domovoi-hub/src/api/capability_ceiling.rs`: no `SettingsRepo`, `.set(`, `AgentLink`, `agent.toml`, no `post`, `put`, `delete` or `patch` route; the `#[utoipa::path]` is `get`; the generated OpenAPI has no non-GET operation under `/hosts/{name}/ceiling` |
| `openapi_snapshot` | regenerated and read |

### 12.2 Web: Capability Center, Host limits, Helper (`src/features/capabilities/*.test.tsx`)

| Test | Setup, action, expected |
|---|---|
| `inventory_is_the_default_tab_and_25s_tests_still_pass` | `/capabilities`: tablist with Inventory selected, prompt 25's list visible; the existing `capabilities.test.tsx` passes unchanged |
| `tab_choice_lives_in_the_url` | choose Packages: location is `/capabilities?tab=packages`; Inventory (first tab) has no parameter; a reload keeps the tab; an unknown `tab` falls back to Inventory |
| `inventory_rows_carry_their_visibility_categories` | rows have `data-visibility-id` `capabilities.row.state` (P0) and `.technical` collapsed in Standard |
| `limits_tab_shows_the_entries_the_host_reported` | reported answer; table with Capability, Targets (`Any target` tag and name tags), Per hour, Active at once, Longest hold ("1 h"), Argument bounds (`min_prefix_len_v4: 24`) |
| `limits_tab_is_read_only` | reported answer; no textbox, checkbox, switch, "Save" or "Edit"; only the `calls` GET to the ceiling and hosts routes were made; source scan: `LimitsTab`, `LimitsTable`, `limitsText` contain no `useMutation`, `.mutate(`, `api.POST`, `api.PUT`, `api.DELETE` |
| `ceiling_not_reported_says_so_and_does_not_imply_permission` | `not_reported`, connected; text equals `ceilingNotReported(host)`; neither "allowed" nor "no limits" appears |
| `ceiling_not_reported_for_an_offline_host_says_the_host_is_offline` | `connected: false`; the offline sentence |
| `ceiling_unreadable_is_not_permissive` | `unreadable`; warning with the reason and "treated as not allowed"; no table |
| `ceiling_disabled_summary_says_nothing_runs_without_approval` | reported, `unattended_enabled: false`; the sentence of 6.8 |
| `ceiling_hash_is_collapsed_metadata` | in Standard the fingerprint sits behind "+N details"; the full value is in a tooltip |
| `limits_tab_asks_for_a_host_under_all_hosts` | no host selected; the chooser, no ceiling request |
| `limits_tab_load_error_is_not_an_empty_state` | 500; `LoadError` with Retry, which refetches |
| `helper_views_are_labelled_placeholders_with_no_requests` | render the Helper tab and `HelperKeyPage`: title "Not available on this hub yet", the two fixed bodies, a "Not available yet" tag; `calls` is empty; no "enrolled", "pinned", "verified", "covered" |
| `helper_key_page_links_back_to_the_helper_tab` | the link target is `/capabilities?tab=helper` |
| `capability_center_has_no_helper_data_path` (gate) | scan `src/api/**` and `src/features/capabilities/Helper*`: none mentions `/helper`, `attest`, `safety_attestation`, `helper_status`, `enroll_helper`; failure text "when the hub gains helper endpoints, replace the placeholder and update this test" |

### 12.3 Web: Packages tab, import, store, installed, gates (`src/features/packages/*.test.tsx`)

| Test | Setup, action, expected |
|---|---|
| `packages_tab_explains_when_installation_is_off` | status `enabled: false`: info note, a link to `/settings/packages`, Import and Install disabled with reasons, the list still shown |
| `packages_tab_warns_when_no_owner_key_is_set` | enabled, `owner_keys: []`: warning note |
| `owner_keys_are_read_only_text` | owner and revoked keys listed as text with the sentence of 6.7.1; no button, input or link that edits a key |
| `import_sends_archive_and_signature_as_multipart` | choose `nginx-tools-1.2.0.tar.zst` and `.sig.toml`; Import: the captured `bodySerializer()` is a `FormData` with exactly `archive` and `signature` and their file names; no password was asked |
| `import_needs_both_files` | one file: Import disabled with the reason |
| `import_shows_the_signature_result_and_how_to_continue` | `201`: title "Imported nginx-tools 1.2.0", signer label, key id, short digest, `SIGNATURE_NOTE`, an "Install it" link with `?package=&version=`; files cleared |
| `import_reports_an_existing_package_without_alarm` | `200` with `already_imported: true`: "already in the store" |
| `import_failures_read_in_words` | table-driven over `unknown_key`, `revoked_key`, `bad_signature`, `digest_mismatch`, `signature_file_invalid`, `not_zstd`, `too_large`, `store_full`, `package_conflict`, `packages_disabled`: each shows a plain sentence, never the raw code, the files stay chosen |
| `import_warns_when_the_two_files_do_not_match_by_name` | differing stems: warning, Import still enabled |
| `store_list_has_loading_empty_error_states` | skeleton; "No packages yet" with the next step; a 500 is a `LoadError`, not "No packages yet" |
| `missing_package_file_is_flagged_and_blocks_install` | `file_status: missing` (if on the list DTO, else in details): pill "File missing", Install disabled with the reason |
| `details_dialog_lists_digest_signature_and_hosts` | digest and signature file in `CopyBlock`s; hosts with status pills and "as last reported" |
| `delete_package_is_confirmed_and_says_installed_copies_stay` | `DELETE` only after the dialog's confirm; the consequence sentence; `409 package_in_use` reads in words inside the dialog |
| `installed_list_is_display_only_and_says_when_it_was_last_checked` | host packages with `observed_at`: status pills, boundary pill, "Last checked", "Installed does not mean enabled." |
| `check_host_refreshes_and_reports_in_words` | click: `POST .../refresh`, list replaced; a not-connected failure reads through `describeError` |
| `installed_list_says_when_the_agent_has_not_turned_installation_on` | `install_allowed: false`: the info note |
| `recent_installs_link_to_the_job_page` | rows with status pills; clicking opens `/capabilities/jobs/{id}` |
| `job_mutations_have_exactly_two_call_sites` (gate) | `import.meta.glob` source scan of `src/**` (not tests): `useCreateInstallJob(` appears only in its hook file and `AuthorizeDialog.tsx`, `useRetryTarget(` only in its hook file and `RetryDialog.tsx` |
| `package_screens_show_no_hub_path` | render details with a fixture: no `/var/`, `/etc/` hub path, no `.part`, no `data_dir` |

### 12.4 Web: wizard and job page (`src/features/packages/wizard*.test.tsx`, `job*.test.tsx`)

| Test | Setup, action, expected |
|---|---|
| `wizard_state_decides_the_first_step_and_what_blocks_next` (pure) | `initialState` with and without `package` and `version`, with a `host` and with a selected host; `canContinue` returns each reason of 6.7.3; `MAX_HOSTS` |
| `wizard_walks_package_hosts_review_confirm_to_the_job_page` | prefilled `?package=&version=&host=`: hosts step with the host ticked; Next shows the review with the plan code, host row, notes; "Review and confirm" opens the dialog; typing the name and password and confirming sends one `POST /packages/jobs` and navigates to `/capabilities/jobs/{id}` |
| `wizard_never_posts_before_confirmation` | open every step, let the plan load, wait: `calls` contains the plan `POST` but no `POST /api/v1/packages/jobs` and no retry or refresh |
| `wizard_offers_no_automation` | on every step: no switch, and no checkbox or button whose accessible name matches `/automatic|auto-?install|auto-?update|schedule|recurr|every host|all hosts|remember|policy|unattended/i` (the host checkboxes are named by host name, and the password input's `autoComplete` attribute is not a name); source scan of `src/features/packages/` for the identifiers `autoInstall`, `autoUpdate`, `autoApprove`, `schedule`, `recurring`, `unattended` and `policy` (none) |
| `hosts_step_offers_only_hosts_with_agents_and_says_what_offline_means` | a hub-only host disabled with its reason; offline host note with the approval window minutes; incompatible host note |
| `hosts_step_limits_the_count` | 0 and 17 hosts: Next disabled with the reasons |
| `review_shows_availability_installed_version_and_warnings_per_host` | two hosts, one `available`, one `host_incompatible` with a warning: pills and sentences from prompt 25's vocabulary |
| `review_blocks_confirm_when_no_host_can_run_it` | all hosts `not_installed`: the button's `disabledReason` |
| `review_shows_the_full_fingerprint_only_as_collapsed_detail_in_standard` | Standard profile: "Package fingerprint" collapsed; the summary and the plan code stay |
| `authorize_needs_password_and_typed_package_name` | confirm disabled with "Enter your password first." then "Type nginx-tools to confirm."; enabled only with both; the body has `password`, `confirm_text`, `plan_hash` |
| `authorize_sends_the_hash_captured_when_the_dialog_opened` | the plan refetch changes the hash after the dialog opened: the sent `plan_hash` is the captured one |
| `plan_change_under_an_open_dialog_blocks_confirm` | the plan query returns a new hash while the dialog is open: confirm disabled with the reason; nothing sent |
| `password_is_not_kept_after_submit` | submit (success and a failure case); the input is empty, and no entry in `queryClient.getMutationCache()` holds the password |
| `confirm_failures_stay_in_the_dialog_in_words` | `403`, `429` (with the countdown), `409 host_busy` (host named), `409 plan_changed` (dialog closes, "The plan changed. Review it again." shown, plan refetched), `409 packages_disabled` |
| `uninstall_flow_has_no_artifact_and_says_what_is_removed` | `kind=uninstall`: no digest or signer; the consequence says it is removed; the confirm button reads "Remove" and is a danger button |
| `job_page_polls_while_running_and_stops_when_finished` | fake timers: a running job refetches every `JOB_POLL_MS`; after it becomes `succeeded` no more requests; a hidden/unknown status does not poll |
| `job_statuses_use_the_vocabularies` | every status of `INSTALL_JOB_STATUSES` and `INSTALL_TARGET_STATUSES` renders its label; a made-up value renders neutral with the raw text |
| `job_targets_show_the_latest_attempt_and_fold_earlier_ones` | host with attempts 1 (failed) and 2 (running): attempt 2 in the row, "Earlier attempts (1)" collapsed in Standard |
| `queued_target_says_it_is_waiting_for_the_host_and_when_it_is_dropped` | countdown to `expires_at` |
| `failed_target_shows_a_sentence_never_a_raw_code` | `failure: {code: "bad_signature", message: "bad_signature: ..."}`; the family sentence and the message without its prefix |
| `every_known_failure_code_has_words` | every entry of `TARGET_FAILURE_CODES` yields a non-empty sentence that does not contain the code; an unknown code uses the fallback title |
| `target_failure_text_is_plain_and_has_no_raw_code` | a message with `<b>` and `**x**`: rendered as literal text |
| `outcome_unknown_says_check_the_host_and_offers_retry_only_as_a_new_approval` | the warning sentence, "Check host" and "Retry"; the page never says "failed" for it; Check host posts the refresh and reports "{host} reports ... installed" or "does not list ..." |
| `succeeded_without_confirmation_says_to_check_the_host` | `outputs_confirmed: false`: the line of 6.7.4 |
| `retry_asks_for_a_fresh_approval` | Retry opens the dialog, which posts a single-host plan, then needs password and typed name; the retry body carries the hash the route compares (6.7.4) and the password; success toasts "Started again on {host}" and invalidates the job |
| `cancel_is_offered_only_for_queued_targets_and_needs_no_password` | queued: Cancel posts without a password; other statuses: no Cancel button; a 409 shows inline |
| `job_page_404_says_no_such_install_with_a_link_back` | `failure(404, "no_such_job", ...)` |
| `unknown_values_render_neutral_and_never_as_success` | unknown availability, target status and reporting values: neutral pill, raw text, no success tone, no confirm enabled by them |
| `copy_never_claims_enabled_or_available_early` | render the five screens and the dialogs; none of the phrases of 6.10 appears |

### 12.5 Visibility and gates (`src/features/capabilities/visibility.test.tsx`, `src/features/packages/gates.test.ts`)

| Test | Setup, action, expected |
|---|---|
| `p0_visible_in_every_profile` (cases added to prompt 37's helper) | `expectP0Everywhere` over: Inventory tab, Packages tab (off and on), Host limits tab (reported and unreadable), Helper tab, wizard review, job page with a failed and an unknown-result target; expected ids from 6.6 |
| `categories_match_the_table` | each id of 6.6 appears with the category the table gives |
| `no_p0_inside_a_collapsed_container` | provided by `expectP0Everywhere`'s ancestor check |
| `visibility_never_gates_package_authority` (gate) | source scan: `AuthorizeDialog.tsx`, `RetryDialog.tsx`, `wizardState.ts`, `jobModel.ts` and the four hook files contain none of `useVisibility`, `useProfile`, `Visible`, `presentationFor`, `DetailsScope`, `visibilityAttrs` |
| `capability_center_has_no_authority_controls` (gate) | scan `src/features/capabilities/` for `api.POST`, `api.PUT`, `api.DELETE`, `.mutate(` and `useMutation` (none are allowed there), and `src/features/packages/` for the same outside the call sites listed in 12.3; neither folder mentions `trusted_keys` or `allow_install` outside the owner-facing text, and the status DTO's `owner_keys` and `revoked` are only read |

### 12.6 Design catalogue and vocabularies

- `src/design/tone.test.ts` (existing) walks the seven new tables through `VOCABULARIES`; it passes with a label,
  tone, hint and icon for each. `messages.test.ts` runs the fifteen new codes through its existing `it.each`.
- `src/dev/CapabilityDemos.tsx` shows every status of the seven tables as pills, a plan review, a job row in each
  target status, a limits table and the two placeholders, in both themes; `capabilityDemos.test.tsx` renders them
  (axe runs; no `skipAxe`).

### 12.7 Smoke test and screenshots (prompt 30's `scripts/`)

- `smoke/server.mjs` lists, in addition to prompt 30's routes: `GET /api/v1/hosts/{name}/capabilities`,
  `GET /api/v1/hosts/{name}/ceiling`, `GET /api/v1/packages/status`, `GET /api/v1/packages`,
  `GET /api/v1/packages/{name}/{version}`, `POST /api/v1/packages/import`, `POST /api/v1/packages/jobs/plan`,
  `POST /api/v1/packages/jobs`, `GET /api/v1/packages/jobs`, `GET /api/v1/packages/jobs/{id}`,
  `GET /api/v1/hosts/{name}/packages`, `POST /api/v1/hosts/{name}/packages/refresh`. **No helper route.** Any other
  `/api` request still fails the run. `data.mjs` builds the payloads (a package, a plan for two hosts, a job whose
  second `GET` returns the finished state).
- New checks appended to prompt 30's nine (each prints one line; first failure exits 1 with a screenshot):
  10. `/capabilities?tab=packages`: the store lists the package; import two files and see "Imported"; the "Install
  it" link opens the wizard with the package chosen. 11. Wizard: choose the host, review shows `Plan xxxxxxxx`,
  confirm is refused until the password and the typed name are entered, and the captured `POST /packages/jobs` body
  has the full plan hash, then the job page shows Running and, after the next poll, Succeeded **without a reload**.
  12. `/capabilities?tab=limits&host=nas` shows the reported limits and no editable control; `?tab=helper` shows
  "Not available on this hub yet" and the run recorded no helper request. 13. Real-browser axe (including colour
  contrast) on the Packages tab, the wizard review step, the job page, the limits tab and the helper tab; no
  horizontal scroll at 375 px on each; no `console.error`.
- `scripts/screens.mjs`: add `["capabilities", "/capabilities"]`, `["capabilities-packages",
  "/capabilities?tab=packages"]`, `["capabilities-limits", "/capabilities?tab=limits&host=nas"]`,
  `["capabilities-helper", "/capabilities?tab=helper"]`, `["package-install", "/capabilities/install"]`. Against
  the dev hub these show the "installation off", empty and "not reported" states; populated states are covered by the
  smoke test. Look at the pictures at four widths in both themes.

## 13. Acceptance criteria

1. `/capabilities` is the Capability Center with tabs Inventory (prompt 25's list, moved not copied, its tests
   unchanged), Packages, Host limits and Helper; the tab is in `?tab=`.
2. A signed package and its signature file can be imported with the two-field multipart request; the result shows
   the signer and the "checked by this hub, checked again by each host" sentence; every hub refusal reads in words.
3. The install wizard walks package, hosts, review, confirm and progress; nothing is sent before the confirm button;
   the confirm needs the password and the typed package name and sends the full plan hash captured when the dialog
   opened; a changed plan blocks confirm; the password is never kept.
4. The job page shows each host's current attempt with the status vocabularies, polls only while a host is working,
   and offers Retry (a fresh approval), Cancel (queued only) and Check host exactly as 6.7.4.
5. Uninstall uses the same flow and says what is removed.
6. No screen offers or implies automation: no schedule, auto-install, policy or chat path; the two job-mutating hooks
   have exactly two call sites.
7. The Host limits tab shows what the host reported, "not reported" (offline or silent) or "unreadable", with
   the sentences of 6.8, and contains no control or request that could change a ceiling; `GET
   /api/v1/hosts/{name}/ceiling` is read-only and tested.
8. The Helper tab and `/capabilities/helper-key` are labelled placeholders with no request, no data, no DTO and no
   helper words other than "Not available"; section 6.9 lists what the helper prompt must add; the placeholder
   gate test exists.
9. The seven vocabularies, the fifteen hub codes and every known failure code have words; unknown values render
   neutral; no screen says a package is enabled, ready or running, or claims the helper or runner is available.
10. Every element carries a category per 6.6; `capabilities` is in `VISIBILITY_PAGES`; `p0_visible_in_every_profile`
    passes for the listed screens; no approval or request code reads the profile.
11. A Settings section "Packages" edits `packages.enabled` and `packages.approval_window`; the Packages tab says the key
    lists and size limits are file-only.
12. No file under `src/features`, `src/app` has a raw colour, inline style, native `title`, raw `<button>` or more than
    250 counted lines; icons only through `Icon`; statuses only through `tone.ts` lookups; error text only from
    `messages.ts`.
13. `cd web && npm run format:check && npm run lint && npm run typecheck && npm test && npm run build && npm run smoke`
    and the hub checks of step 14 pass; `gen:api` shows no further diff; the bundle stays inside the budget.
14. The docs of section 15 are written; no committed file mentions prompts, the build plan or agents.

## 14. Migration and compatibility

- **Existing installs.** Package installation is off until the owner turns it on, so a new install sees the Packages
  tab with the "off" note and an empty store; nothing else changes. `/skills`, the host pages and prompt 25's
  inventory behave as before. No data migrates, no setting is renamed.
- **Hub without prompt 42 or 25.** Not supported; step 1 stops if either is missing.
- **Agents.** Unchanged. A host whose agent has not turned on package installation shows `not_advertised` words.
- **The helper prompt (later).** It replaces the two placeholder bodies and the placeholder gate test (6.9); the tab id
  `helper`, the route `/capabilities/helper-key`, the file names `HelperTab.tsx`, `HelperPlaceholder.tsx`,
  `HelperKeyPage.tsx`, `HelperKeyPlaceholder.tsx` and the `capabilities.helper*` ids are the stable mount points.
- **Other U4 pages (43, 48, 50, 51).** They append their own page keys to `VISIBILITY_PAGES` and their own routes; they
  do not edit this prompt's files.

## 15. Documentation requirements

Write for the product's readers; never mention prompts, prompt numbers, the build plan or agents in a committed file
(except `AGENTS.md`, which may name them).

- **`web/DESIGN.md`** (same PR): (a) "Components": nothing new in the kit; under "Layout patterns" add the
  **wizard pattern** (stepper, one decision per step, a review that shows the plan code, a confirm dialog with
  password and typed name, then a progress page) and the **read-only host view pattern** (what the host reported,
  who owns it, how to change it, never a control). (b) "Add a status": add install target, install job, host package,
  key state, file status, privilege boundary and ceiling reporting to the list. (c) "How people use it": a short
  "Install a package" paragraph: import, choose hosts, review the plan code, confirm with password and the package
  name, watch each host, retry needs a fresh approval, installed is not enabled. (d) "Planned": remove the owner
  package import and the Capability Center install wizard sentence now delivered; **keep** the helper views, the
  attestation, the enrollment screen and the privilege-state banners, and add one line: "The Capability Center
  reserves the Helper tab and `/capabilities/helper-key` as labelled placeholders until the hub collects helper
  data." (e) "Checks that will fail your PR": add the placeholder gate (`npm test`, fix: when the hub gains helper
  endpoints replace the placeholder and update the test) and the two-call-site gate for install jobs. (f)
  "Performance budget": record the new `npm run size` numbers.
- **`AGENTS.md`** (committed; keep in step), under "Web UI": one bullet: package install jobs are created or retried only
  from the confirm button of the Capability Center's authorization dialogs (password plus typed package name); never
  add automation, a chat path or a policy path for them; helper and key-enrollment views stay placeholders until the
  hub has helper endpoints.
- **Book.** New page `docs/book/src/guide/packages.md` ("Install a package"): what a package is and who signs it; the two
  files; turning installation on (Settings, Packages, and the file-only owner key list with a TOML example of
  `[[packages.owner_keys]]` copied from the config reference); importing; the wizard step by step with the password and
  the typed name; reading a job (each status in one line, retry, cancel, check host, "result unknown"); installed is
  not enabled; each host decides which keys it trusts and the hub cannot change them. Link it from `SUMMARY.md` under
  Guides. Add to prompt 25's `docs/book/src/concepts/capabilities.md` a section "Host limits": what the unattended
  ceiling is, that the host owns it, how to read the tab, "not reported" and "unreadable", and how to change it on the
  host. Add the ceiling endpoint to `docs/book/src/reference/api.md` in the hosts section, in the same shape as its
  neighbours. Run `cargo xtask docs-links`.
- **`ARCHITECTURE.md` §10 (HTTP API and web UI)**: add the read-only ceiling endpoint and one sentence naming the
  Capability Center pages. No other section changes (no crate, port or wire change).
- **OpenAPI**: `cargo xtask openapi && cd web && npm run gen:api`; both generated files and the snapshot are committed,
  never hand-edited. No config-reference regeneration (no config key added).
- **Doc comments**: every new public Rust item says what it is for; every new web component has a JSDoc with "for" and
  "not for".


## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
