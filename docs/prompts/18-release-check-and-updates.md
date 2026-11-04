# 18 · Feature: tell the user when a new release is out, and how to update

Two prompts. 18a is the notice; 18b (optional) adds a `domovoi upgrade` command. A fully
automatic updater inside the hub is **not** recommended: the hub runs unprivileged
(`domovoi` user, sandboxed systemd unit) and cannot replace `/usr/local/bin/domovoi` or
restart services, and Domovoi never spawns processes outside the runner (rule R3).

## Background (verified)

- Releases are GitHub releases of `RoastSlav/domovoi`. Install/upgrade commands are in
  `docs/book/src/install/upgrading.md`:
  `curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub`
  then `sudo systemctl restart domovoi-hub` (agents: `--role agent`, `domovoi-agent`);
  containers: `docker compose -f hub.yml pull && docker compose -f hub.yml up -d`.
  `install.sh` takes `--version vX.Y.Z`.
- Archives are `domovoi-<tag>-<target>.tar.gz` with a `.sha256` next to them, targets
  `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (`install.sh`).
- The hub already makes outbound HTTPS calls with `reqwest` (heartbeat in
  `crates/domovoi-hub/src/runtime/schedulers.rs`, user agent `domovoi/<version>`).
- Periodic hub jobs live in `schedulers.rs` (427 lines: add only a few lines there; put
  logic in a new file). `every(clock, interval, shutdown, f)` is in `runtime/every.rs`.
- `HostDto.outdated` already marks agents older than the hub.
- `crates/domovoi-config/src/hub/mod.rs` is 462 lines: put a new config section in its
  own file under `crates/domovoi-config/src/hub/`.
- If prompt 17 is done, use `domovoi_proto::version::parse_release` to compare versions;
  otherwise use `is_older` in `crates/domovoi-hub/src/api/version.rs`.

---

## 18a · Release check, banner and update instructions

### Steps

1. **Config.** New `crates/domovoi-config/src/hub/updates.rs`:
   `UpdatesSection { check: bool /* default true */ }` documented as "Look for a newer
   Domovoi release on GitHub twice a day. Nothing is sent but the request itself."
   Add `#[serde(default)] pub updates: UpdatesSection` to `HubConfig`. Add the field to
   the Settings UI (`web/src/features/settings/sections.ts`, a new "Updates" section with
   one bool field `updates.check`).
2. **Checker.** New `crates/domovoi-hub/src/runtime/updates.rs`:
   - `pub struct UpdateInfo { current, latest, newer: bool, url, notes, published_at, checked_at }`
     (serde, all `String`/`Option<String>`, `checked_at` a `Timestamp`).
   - A pure `pub fn evaluate(current: &str, release: &serde_json::Value, now: Timestamp) -> Option<UpdateInfo>`
     reading `tag_name`, `html_url`, `body` (cut to 4000 chars), `published_at`; ignore
     drafts and prereleases (`draft`, `prerelease` true).
   - `async fn check(http, url) -> Result<Value, String>`: `GET` the URL with header
     `Accept: application/vnd.github+json`, 10 s timeout.
   - The latest result is kept in an `Arc<RwLock<Option<UpdateInfo>>>` and saved with
     `SettingsRepo::set(store, "update_check", ..)`; load it at start.
   - URL constant `https://api.github.com/repos/RoastSlav/domovoi/releases/latest`.
3. **Schedule.** In `Schedulers::spawn`, when `config.updates.check`, spawn the checker
   every 12 hours, first run 1 minute after start. Failures log at `debug` (offline hubs
   are normal) and keep the previous result.
4. **API.** `GET /api/v1/system/update` → `UpdateDto` with the stored info plus ready
   commands for the latest tag: `hub_command`
   (`curl -fsSL .../releases/download/<tag>/install.sh | sudo sh -s -- --role hub --version <tag> && sudo systemctl restart domovoi-hub`),
   `agent_command` (same with `agent` / `domovoi-agent`) and `docker_command`. And
   `POST /api/v1/system/update/check` (runs a check now; at most once a minute, else
   429) → `UpdateDto`. Add the POST to the `writes` list in
   `crates/domovoi-hub/tests/gates.rs`. The checker handle goes into `ApiDeps`
   (update the three construction sites). Regenerate OpenAPI.
5. **Web.** Hook `useUpdate()` (refetch every hour). New
   `web/src/features/updates/UpdateBanner.tsx`, rendered in `Layout.tsx` above
   `<SetupBanner />`: when `newer`, show "Domovoi {latest} is available (this hub runs
   {current})." with **What's new** (opens a `Dialog` with the notes in `Markdown` and a
   link to `url`) and **How to update** (a `Dialog` with three `CopyBlock`s: hub, each
   agent, Docker, plus "Update the hub first, then the agents."). A **Dismiss** button
   hides it for that version (`localStorage` key `update-dismissed:<latest>`, wrapped in
   try/catch). On the Hosts page, outdated agents get a "How to update" link opening the
   same dialog.
6. **Settings → About.** Show the current version, the last check time, and a
   **Check now** button.
7. **Docs.** `docs/book/src/install/upgrading.md`: the banner, the setting, and how to
   turn it off (`[updates] check = false` or `DOMOVOI_UPDATES__CHECK=false`).

### Tests

- `evaluate()`: newer tag → `newer: true`; same → false; prerelease ignored; missing
  fields → `None`.
- API: `GET /api/v1/system/update` with a stored result returns commands containing the
  tag.
- Web: banner shows when newer, hides after Dismiss, dialog shows the commands.

---

## 18b · Optional: `domovoi upgrade`

### Steps

1. New subcommand in `crates/domovoi/src/cmd/upgrade.rs` (register in `cmd/mod.rs`):
   `domovoi upgrade [--version vX.Y.Z] [--dry-run]`. Run it with `sudo`.
2. It finds the latest tag (or uses `--version`), picks the target from
   `std::env::consts::ARCH` (`x86_64`/`aarch64`, Linux only), downloads the archive and
   its `.sha256`, verifies the SHA-256 (`sha2` is already a workspace dependency),
   extracts `domovoi` from the archive (add `flate2` and `tar` to
   `[workspace.dependencies]` and to `crates/domovoi/Cargo.toml`; check
   `cargo deny check` passes), writes it next to the current executable
   (`std::env::current_exe()`) as `domovoi.new` with mode 755, then renames it over the
   old one.
3. It never restarts anything itself (rule R3). It prints the exact next commands:
   `sudo systemctl restart domovoi-hub` and/or `domovoi-agent`, depending on which unit
   files exist in `/etc/systemd/system/`.
4. `--dry-run` prints what it would download and replace, and changes nothing.
5. The update dialog from 18a shows `sudo domovoi upgrade && sudo systemctl restart …` as
   the first option when the hub runs a binary install.

### Tests

- Unit-test target selection, checksum mismatch (refuses, leaves the old binary), and
  extraction from a small tarball built in the test.

### Done when

- 18a: a newer release shows a dismissible banner with notes and exact commands; the
  check can be turned off.
- 18b: `sudo domovoi upgrade` replaces the binary safely and tells the user what to
  restart.
