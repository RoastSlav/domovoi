# 0017. Manage settings from the web UI

- Status: Proposed
- Date: 2026-11-01

## Context

Today every setting lives in `hub.toml` and `agent.toml` (§14). Secrets must be referenced indirectly as `{ env = .. }` or `{ file = .. }`, and every change needs a file edit and a restart. The settings page in the web UI is read-only. Getting a working install means editing TOML, creating secret files with the right permissions and restarting a service. For a self-hosted tool aimed at people replacing a cron-and-script monitor, that is the hardest part of the first hour.

The goal is that someone who installs Domovoi never has to open a file. Install, open the setup URL, create the admin account, connect Discord and a model, enroll a host. People who manage their machines as code must still be able to drive everything from files and environment variables.

The `settings` table and the `SettingsRepo` port already exist (ADR 0015 uses them for template overrides). Agents already accept `config_update` with per-check overrides (§8).

## Decision

### Two tiers of settings

- **Bootstrap settings** are needed before the store opens, or could make the UI unreachable if set wrong: `hub.data_dir`, `web.bind` and `agent_server.bind`. They come only from the command line, `hub.toml` and `DOMOVOI_*` variables, and they have working defaults. The UI shows them read-only.
- **Everything else** in `HubConfig` can be edited from the UI: hub name and time zone, public URLs, trusted proxies, LLM providers and prices, triage limits, incident and notification policy, Discord, webhooks, probes, heartbeat, backups and secret hygiene.
- `hub.toml` becomes optional. `domovoi hub run` with no `--config` starts on defaults and the first-run setup URL.

### Precedence and provenance

- Layers, lowest to highest: built-in defaults, then values stored from the UI, then `hub.toml`, then `DOMOVOI_*` variables.
- The loader records which layer set each field. The API returns that source per field. A field set in `hub.toml` or the environment is locked in the UI, with a label naming the file key or variable. Someone who manages config as code never sees the UI quietly override it, and someone who uses the UI never needs the file.
- Layering happens on the TOML value tree before deserializing, so the merged result goes through the same `Deserialize`, `Validate` and error paths (ADR 0013) as a file.

### Storage

- UI values are stored as one JSON document in the `settings` table under the key `config`, shaped like `hub.toml`, with a `revision` counter. Saving is all-or-nothing. A save sends the revision it started from and is refused with `409 Conflict` if the stored revision has moved on.
- The last successfully applied document is kept under `config.previous` for rollback.
- Per-host check overrides are stored under `host.<name>.checks` and sent to the agent in `config_update` on save and at every handshake. The agent applies them live.

### Secrets

- Secret fields in the API are write-only. A request carries `{ "keep": true }`, `{ "set": "<value>" }` or `{ "clear": true }` for each one. Responses keep showing only `[set]` or `[unset]`, plus the source. INV-5 is unchanged.
- Secrets entered in the UI are encrypted at rest with ChaCha20-Poly1305 (`ring::aead`). The key is 32 random bytes in `<data_dir>/secret.key`, mode `0600`, created at first start and never stored in the database. The secret's config path is the associated data, so a ciphertext cannot be moved to another field. Database backups contain only ciphertext.
- `SecretSource` gains a `Stored` variant. `Debug` prints `stored` and never the value. The redactor registers stored secrets like every other resolved secret.
- If the key file is missing or wrong, the affected secrets resolve as unset, a `StoredSecretUnreadable` advisory names them, and the UI asks for them again. The hub still starts.
- File secrets keep the `{ env = .. }` / `{ file = .. }` rule. Inline secrets in `hub.toml` are still rejected.

### Applying a change

- The API merges the candidate with the other layers, validates the whole result and returns every issue with its path, so the form shows each error next to its field. Nothing is stored unless validation passes.
- A valid save is applied by restarting the hub runtime in the same process: the `hub run` supervisor cancels the running `HubRuntime`, builds a new one from the merged config on the same store, and starts it. This is the same code path as a cold start. Agents reconnect and flush their spool. The SPA shows "Applying…" and reconnects when `/healthz` answers.
- If the new runtime fails to build, the supervisor restores `config.previous`, starts on it, raises a `SettingsApplyFailed` advisory with the error, and the API reports the failure.
- Changes that need no runtime restart (per-host check overrides, and template overrides from ADR 0015) are applied without one.

### Testing a connection before saving

- `POST /api/v1/settings/test/{discord,llm,heartbeat}` takes candidate values, which are not saved, and sends one test message, makes one minimal model call, or sends one ping. It returns a short result. These endpoints are rate-limited and write nothing.

### Who can change settings

- Only an authenticated admin, through the web API, with a CSRF token (INV-8). No LLM tool, chat command, webhook or agent message can read secrets or write settings. The LLM still never produces a config change (principle 1). A gate test asserts that no `ReadTool` variant and no chat command reaches the settings service.
- Changes that widen who can act or what is trusted need the admin's password again: `discord.allowed_user_ids` (who may approve from chat), `web.trusted_proxies`, webhook definitions and any secret. The password check shares the login rate limit.
- Every save writes a `settings.changed` audit event. The event lists the changed paths with old and new values for plain fields, and only `set` or `cleared` for secrets.

### Agents

- `agent.toml` keeps only what the agent needs to reach the hub and read the host: identity, hub URL and pinned fingerprint, data dir, `host_root` and skills dir. `domovoi agent enroll` writes it, so nobody has to edit it.
- Check enablement, intervals and thresholds are managed per host from the UI through `config_update`. Local `agent.toml` values remain the base those overrides apply to.
- Skill manifests stay files on the agent host. The hub must never push executable content to an agent (INV-3). That boundary is a deliberate exception to "never edit a file".

### Changes to `ARCHITECTURE.md`

§14 becomes:

> - Settings come from four layers, lowest first: built-in defaults, values saved in the web UI (`settings` table), `hub.toml`, `DOMOVOI_*` environment variables. Each field records its source; fields set in a file or the environment are read-only in the UI.
> - Bootstrap settings (`hub.data_dir`, `web.bind`, `agent_server.bind`) come only from the command line, file or environment. `hub.toml` is optional.
> - Secrets in files use `{ env = .. }` or `{ file = .. }`. Secrets entered in the UI are encrypted at rest with a key in `<data_dir>/secret.key`, are write-only through the API and are held as `secrecy::SecretString`.
> - A settings change is validated as a whole, audited, and applied by an in-process restart of the hub runtime, with rollback to the last good settings if the restart fails.
> - `domovoi check config <file>` validates without starting and reports every problem with its TOML path.
> - Config structs derive `schemars::JsonSchema`; the reference documentation and the UI's field help are generated from them.

## Consequences

- No file editing is needed to install and run Domovoi. The installer and docs point to the setup URL instead of "edit `/etc/domovoi/hub.toml`".
- `domovoi-config` gets a layered loader that returns the config with a field-to-source map, and the `Stored` secret source. It stays free of store access: the hub passes in the stored document and the decrypted secrets.
- `ring` becomes a direct workspace dependency. It is already in the tree through rustls, so `cargo deny` sees nothing new.
- The hub gets a settings service (merge, validate, encrypt, store, audit, apply) and endpoints, and the binary's `hub run` becomes a supervisor loop around `HubRuntime`.
- The agent must apply `check_overrides` from `config_update` live. Today it applies only the heartbeat interval.
- Losing `secret.key` loses UI-entered secrets, but not the rest of the data. The backup docs must say to keep the key with the backups but apart from them.
- Applying a change restarts the runtime. The UI drops its SSE connection and agents reconnect. Brief, but visible. Hot reload per component can replace it later without changing the API.
- New gates: `settings_never_return_secret_values` extends to the write path and the test endpoints, `stored_secrets_are_encrypted_at_rest`, `settings_writes_require_admin_session_and_csrf`, `no_tool_or_chat_command_reaches_settings`.

## Alternatives considered

- Writing UI changes back into `hub.toml`: loses comments and formatting, `/etc/domovoi` is read-only under the hardened unit (`ProtectSystem=strict`), and secrets would have to be written inline, which §14 forbids.
- Settings only in the database, no file layer: breaks people who deploy with Compose and environment variables or manage config as code.
- UI values win over the file: a file change would silently do nothing, which is worse than a locked field with a label.
- Storing secrets in the database unencrypted: every backup copied off-box would carry the Discord token and API keys in plain text.
- Hot reload in each component: many reload paths to get right and test. A runtime restart reuses the start-up path that every test already covers.
