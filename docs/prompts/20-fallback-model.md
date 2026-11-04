# 20 · Feature: fallback models from the same provider

Do prompt 19 first.

## Goal

Each provider can list **fallback models** (`llm.primary.fallback_models`,
`llm.secondary.fallback_models`). When a model fails, triage tries the next model of the
same provider, then the fallback provider and its models. Example: `claude-opus-5-5`,
then `claude-sonnet-5-5`, then a local Ollama model.

## What is already known (verified)

- `LlmProvider` config (`crates/domovoi-config/src/hub/llm.rs`) has one `model`.
- Adapters are built from `ProviderSettings` by `domovoi_llm::build(&settings, executor)`;
  `crate::runtime::settings::provider(&LlmProvider) -> ProviderSettings` in
  `crates/domovoi-hub/src/runtime/settings.rs` copies `model` into it.
- The engine takes `TriageDeps { primary, secondary: Option<..> }`
  (`crates/domovoi-engine/src/triage/mod.rs`); the chain is built twice:
  `std::iter::once(&deps.primary).chain(deps.secondary.as_ref())` in `triage/run.rs`
  (`providers()`) and `triage/followup.rs` (`answer()`).
- `secondary:` is set in about 15 places (engine tests harness, drills world, hub
  runtime, hub test harnesses, gates). Find them with
  `grep -rn "secondary" crates --include=*.rs`.
- Provider `id()`s are fixed strings (`anthropic`, `openai_compatible`, `cli`), used for
  usage records and log lines.

## Read these files

- `crates/domovoi-config/src/hub/llm.rs`, `crates/domovoi-config/src/validate.rs`
- `crates/domovoi-engine/src/triage/{mod,run,followup}.rs`
- `crates/domovoi-hub/src/runtime/{mod,settings,start}.rs` (only the LLM parts)

## Steps

1. **Config.** Add to `LlmProvider`:
   `/// Models of the same provider tried in order when `model` fails. Optional.`
   `#[serde(default)] pub fallback_models: Vec<String>,`
   Validation in `validate.rs`: at most 5 entries, none empty, and only for `anthropic`
   or `openai_compatible` (error path `llm.<slot>.fallback_models`, message "fallback
   models need an anthropic or openai_compatible provider").
2. **Engine: a list instead of one secondary.** In `TriageDeps`, replace
   `pub secondary: Option<Arc<dyn LlmProvider>>` with
   `/// Providers tried in order after the primary fails.`
   `pub fallbacks: Vec<Arc<dyn LlmProvider>>`. Add
   `impl Shared { pub(crate) fn chain(&self) -> Vec<&Arc<dyn LlmProvider>> }` returning
   primary then fallbacks, and use it in `run.rs` and `followup.rs`. Update every
   construction site: `secondary: None` → `fallbacks: Vec::new()`,
   `secondary: Some(x)` → `fallbacks: vec![x]`,
   `secondary: setup.secondary.then_some(x)` →
   `fallbacks: if setup.secondary { vec![x] } else { Vec::new() }`.
3. **Hub wiring** (`build_triage` in `runtime/mod.rs`, keep it short; put a helper
   `fn chain_for(config: &LlmProvider, executor) -> Result<Vec<Arc<dyn LlmProvider>>, RuntimeError>`
   in `runtime/settings.rs` or a new `runtime/llm_chain.rs`): for the primary config
   build the primary, then one provider per `fallback_models` entry (same settings with
   `model` replaced). Then the secondary and its fallback models. Overrides from tests
   (`overrides.secondary`) still go first among the fallbacks. Do the same model swap in
   `runtime/start.rs` preflight so a broken fallback model config fails early.
4. **Logs.** When moving to the next provider, log the model too: add
   `fn model(&self) -> Option<&str> { None }` with a default to the `LlmProvider` trait in
   `crates/domovoi-core/src/ports/llm.rs`, implement it in the Anthropic and OpenAI
   adapters, and include it in the fallback log line and the final error text.
5. **Web.** In `sections.ts` add a `list` field "Fallback models" for
   `llm.primary.fallback_models` (and `llm.secondary.fallback_models` if prompt 19 added
   that section), visible for `anthropic` and `openai_compatible`, hint "Tried in order
   when the model above fails. Comma-separated."
6. **Docs.** `docs/book/src/guide/llm-providers.md` and `examples/hub.full.toml`.

## Tests

- Config: validation of `fallback_models` (too many, empty, wrong kind).
- Engine: with `fallbacks: vec![a, b]`, the primary and `a` fail, `b` answers → `Report`
  and three providers were called in order.
- Hub: `chain_for` builds 1 + N providers with the right models (unit test on the
  `ProviderSettings` it produces, before `build`).

## Done when

- Fallback models work for both slots, in order, before and after the fallback provider.
- All existing tests compile and pass with `fallbacks`; new tests pass.
