# 19 · Feature: a fallback AI provider, set up from the web UI

## Goal

In Settings → Models and triage, the user can set up a **fallback provider**
(`llm.secondary`) with its own kind, model, base URL and key, fetch its models, and test
it. Triage and follow-ups switch to it whenever the primary fails for any reason,
including a bad answer.

## What is already known (verified)

- Config and engine already support it: `LlmSection.secondary` in
  `crates/domovoi-config/src/hub/llm.rs`; `TriageDeps.secondary` built in
  `crates/domovoi-hub/src/runtime/mod.rs` (`build_triage`, around line 405); the chain
  primary → secondary is in `crates/domovoi-engine/src/triage/run.rs` (`providers()`)
  and `triage/followup.rs` (`answer()`).
- The engine only falls back on `LlmError::Unavailable | RateLimited | Transport`
  (`run.rs`, `falls_back()`; `followup.rs` has the same `matches!`). It does **not**
  fall back on `Auth`, `InvalidResponse` (an HTTP 404 "model not found" maps to
  `InvalidResponse` in `crates/domovoi-llm/src/http.rs`) or on an unusable report
  (`Failure::Invalid`).
- The settings UI only has `llm.primary.*` fields
  (`web/src/features/settings/sections.ts`, section "Models and triage").
- Connection tests: `Target::Llm` in `crates/domovoi-hub/src/settings/connection.rs`
  always uses `config.llm.primary`; `SettingsService::models()` also uses the primary.
  Routes are in `crates/domovoi-hub/src/api/settings_write.rs` (`TestRequest { changes }`).
  The web hooks are `useTestConnection` / `useModelDiscovery` in
  `web/src/api/hooks/settings.ts`; `ModelField` in `web/src/features/settings/fields.tsx`
  calls discovery.
- `llm.secondary.api_key` is already handled as a secret (`HubConfig::secrets_mut` visits
  both slots).

## Read these files

- `crates/domovoi-engine/src/triage/run.rs`, `triage/followup.rs`
- `crates/domovoi-hub/src/settings/connection.rs`, `crates/domovoi-hub/src/api/settings_write.rs`
- `web/src/features/settings/{sections.ts,SectionForm.tsx,fields.tsx}`,
  `web/src/api/hooks/settings.ts`

## Steps

1. **Engine: fall back on every provider error and on an unusable report.**
   - `run.rs`: `providers()` tries the next provider after *any* `Failure` (provider
     error or `Failure::Invalid`) when one is left, logging
     `tracing::info!(provider, reason, "triage falls back to the next provider")`.
     Delete `falls_back()` or make it return `true`.
   - `followup.rs`: the same: try the next provider after any `LlmError`.
   - The returned error text when all fail lists each provider's reason:
     `"anthropic failed: ...; openai_compatible failed: ..."`.
2. **Hub: test and discover models for either slot.**
   - Add `#[serde(default)] pub slot: Option<String>` ("primary" or "secondary", default
     primary) to `TestRequest` in `settings_write.rs` (document it; it is in the
     OpenAPI body).
   - `connection.rs`: replace `Target::Llm` with `Target::Llm { secondary: bool }` (keep
     the enum `Copy`); pick `config.llm.secondary` when `secondary`, with the message
     "no fallback model is set up" when absent. Give `models()` a `secondary: bool`
     parameter and apply the placeholder-model trick to `llm.secondary` in that case.
   - Pass `req.slot.as_deref() == Some("secondary")` from `test_llm` and `list_models`.
     Regenerate OpenAPI.
3. **Web.**
   - `useTestConnection(target)` and `useModelDiscovery()` accept an optional
     `slot: "primary" | "secondary"` and send it in the body.
   - `ModelField` gets a `slot` prop (default primary) passed to discovery.
   - `FieldSpec` gets an optional `slot` used by `ModelField`.
   - In `sections.ts`, add a section **Fallback model** after "Models and triage", with
     the same fields as the primary but paths `llm.secondary.*` and `visibleWhen` on
     `llm.secondary.kind`, `test: "llm"` and a new `testSlot: "secondary"` on
     `SectionSpec`, plus an intro: "Used when the main model fails, times out, is rate
     limited or gives an unusable answer. Optional."
   - `SectionForm` passes `testSlot` to `useTestConnection` and labels the button
     "Test the fallback model".
4. **Docs.** `docs/book/src/guide/llm-providers.md`: a "Fallback provider" section
   (UI and `[llm.secondary]` in `hub.toml`, when it is used).

## Tests

- Engine (`crates/domovoi-engine/tests/services/`, the harness has
  `Setup { secondary: true, .. }`): the primary returns `LlmError::InvalidResponse`, the
  secondary answers → `Report`; the primary returns a non-JSON answer twice (first turn
  and repair), the secondary answers → `Report`; follow-up falls back on `Auth`.
- Hub: `POST /api/v1/settings/test/llm` with `slot: "secondary"` and no secondary set
  answers `ok: false` "no fallback model is set up".
- Web: the Fallback model section renders, and its test sends `slot: "secondary"`.

## Done when

- The fallback provider can be configured, model-listed and tested from the UI.
- Triage and follow-ups use it after any primary failure.
- All tests pass; OpenAPI regenerated.
