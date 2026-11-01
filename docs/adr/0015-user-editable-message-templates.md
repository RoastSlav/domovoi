# 0015. Let users edit routine message templates

- Status: Proposed
- Date: 2026-11-01

## Context

Routine messages (the daily digest, the quiet-hours batch, resolution notices, the test message) are rendered by fixed Rust code in `domovoi-engine::render`. Users want to change their wording, order and level of detail to suit their setup. Without a variable list they cannot know what data a message has to work with.

Cards that carry an approval are different. Their risk level, destructive-action warning and buttons are part of the approval safety model (ADR 0007) and must look the same every time.

## Decision

- **Scope.** Users can override the templates for `digest`, `quiet_batch`, `resolved_notice` and `test_message`. Incident cards and approval cards are not templatable; at most a later change may add a free-text summary line to them.
- **Engine.** Templates use `minijinja`: sandboxed, no filesystem or network access, loops and conditionals, deterministic output. Rendering stays a pure function in `domovoi-engine`: `(template, context) -> Result<String, RenderError>`.
- **Typed context.** Each templatable message has its own context struct (`DigestContext`, `QuietBatchContext`, `ResolvedNoticeContext`, `TestMessageContext`) that derives `Serialize` and `schemars::JsonSchema`. Only these structs reach a template.
- **Variable catalogue.** The variable list, with a description for each field, is generated from the context structs' JSON Schema, which takes its text from the doc comments. The API serves it at `GET /api/v1/templates/{kind}/variables`, and the web editor shows it beside the template. The list cannot drift from what the template actually receives.
- **Defaults.** The built-in templates ship as `minijinja` files in the binary and produce exactly today's output, protected by the existing snapshots. A user override is stored in the `settings` table under `template.{kind}`. Deleting it restores the default.
- **Validation and preview.** The API parses and renders a template against a sample context before saving and rejects it with the line and column of the error. The editor offers a live preview using that sample context.
- **Failure at send time.** If an override fails to render (for example, a field was removed in an upgrade), the engine sends the message using the built-in template and raises a `TemplateFailed` advisory naming the template and the error. A broken template never silences the digest.
- **Safety.**
  - The `Redactor` runs on the rendered text, after the template.
  - Context structs never contain secrets or raw evidence.
  - Output is truncated to the target platform's limit with a trailing "… (truncated)".
  - Rendering has a fuel limit so a runaway loop cannot stall the scheduler.

## Consequences

- New workspace dependency `minijinja`. It must pass `cargo deny`.
- New `TemplateKind` enum and context structs in `domovoi-core`, template rendering in `domovoi-engine`, endpoints and an editor page in the hub and web UI.
- The context structs become a user-facing contract. Renaming or removing a field is a breaking change, noted in the changelog. Adding a field is not.
- Tests: snapshot tests that the defaults reproduce today's text, a rejected-template test, a fallback-and-advisory test, a redaction-after-render test, and a test that the variable catalogue lists every context field with a non-empty description.

## Alternatives considered

- Handlebars or Tera: Handlebars lacks expressions the digest needs; Tera pulls in more dependencies than `minijinja` and has a weaker sandbox story.
- Plain `{placeholder}` substitution: cannot express "for each host" or "only if there are open incidents", which the digest needs.
- Templating every message, including approval cards: users could remove warnings or risk levels from approval cards, which undermines ADR 0007.
