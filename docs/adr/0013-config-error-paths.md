# 0013. Config errors carry their TOML path via serde_path_to_error

- Status: Accepted
- Date: 2026-10-31

## Context

`domovoi check config` must report every problem with its TOML path (`llm.primary.model`, `probes[0].target`) and say how to fix it. Cross-field validation can name paths itself, but deserialization errors (unknown keys, wrong types, inline secrets) come from serde and the `toml` crate. `toml` reports a byte span, not a path, and environment overrides are applied to the parsed table, so overridden values have no span at all.

## Decision

Add `serde_path_to_error` to the `domovoi-config` allowlist. The loader parses the file into a `toml::Table`, applies `DOMOVOI_*` overrides, then deserializes through `serde_path_to_error`, which records the path of the value being deserialized when an error occurs. `schemars` also enables its `url2` feature so `url::Url` fields derive `JsonSchema`.

## Consequences

- Every deserialization error has an exact path, including errors in values set from the environment.
- One more small, widely used dependency (MIT OR Apache-2.0, by the maintainer of serde), already common in the Rust web stack.
- Deserialization still stops at the first error; secret resolution and validation report every problem they find.

## Alternatives considered

- Hand-written path-tracking `Deserializer` wrapper: no new dependency, but about 300 lines duplicating a maintained crate.
- Mapping `toml` error spans back to paths: fails for values set by environment overrides.
