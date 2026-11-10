# 28 · Security: redact model failure reasons before they leave the engine

## Background (verified on `main`)

- `crates/domovoi-engine/src/triage/followup.rs:200-206` and `triage/chat.rs:224-227`
  build `TriageError::Provider` from `"{provider} failed: {err}"` over the raw
  `LlmError`. `LlmError::Transport(String)` and `InvalidResponse(String)`
  (`crates/domovoi-core/src/ports/llm.rs:126-139`) carry free text from the provider:
  response excerpts, URLs.
- The hub returns that string to the browser: `ApiError::Unavailable(err)` in
  `crates/domovoi-hub/src/api/chat.rs:137`, `api/incidents.rs` (ask) and
  `api/followups.rs:65`; Discord posts it in the thread
  (`auth/interactions/mod.rs:143-146`).
- The answers themselves are redacted (`followup.rs:208`, `chat.rs:228`), and triage
  redacts its failure reason (`run.rs:104`). The question paths do not (INV-5).

## Steps

1. In both `answer` functions, redact the joined reasons once:
   `TriageError::Provider(deps.redactor.redact(&reasons.join("; ")))`.
2. Cut the result to 500 characters with `clip` (`followup.rs:53`) so a long upstream
   body cannot fill a Discord message.
3. Check `TriageError::Store(..)` texts are not returned to the client (they map to
   `ApiError::internal`, which hides them; leave as is).

## Tests (`crates/domovoi-engine/tests/services/redact.rs`)

- `a_failed_follow_up_never_shows_a_secret_from_the_provider_error`: a scripted provider
  fails with `LlmError::Transport("... Bearer abc.def ...")` and a configured secret in
  the text; the `TriageError` text contains `[REDACTED]` and neither value.
- The same for `chat`.

## Done when

- Every string in `TriageError` has passed the redactor.
