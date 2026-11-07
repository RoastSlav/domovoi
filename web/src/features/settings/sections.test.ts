import { expect, test } from "vitest";
import { SECTIONS } from "./sections";

const paths = (title: string) =>
  SECTIONS.find((section) => section.title === title)
    ?.fields({})
    .map((field) => field.path) ?? [];

test("the models section exposes the silence timeout, prices and per-run limits", () => {
  const found = paths("Models and triage");
  for (const path of [
    "llm.primary.timeout",
    "llm.primary.price.input_per_mtok_usd",
    "llm.primary.price.output_per_mtok_usd",
    "triage.cache_days",
    "triage.per_run.tool_calls",
    "triage.per_run.seconds",
    "triage.per_run.output_tokens",
  ]) {
    expect(found).toContain(path);
  }
});

test("the fallback model has the same settings as the main one", () => {
  const primary = paths("Models and triage")
    .filter((path) => path.startsWith("llm.primary."))
    .map((path) => path.replace("llm.primary.", ""));
  const secondary = paths("Fallback model").map((path) => path.replace("llm.secondary.", ""));
  expect(secondary).toEqual(primary);
});

test("every hub setting that can change at runtime has a field", () => {
  const all = SECTIONS.flatMap((section) => section.fields({}).map((field) => field.path));
  for (const path of [
    "agent_server.heartbeat",
    "agent_server.self_check",
    "llm.primary.cli.args",
    "secrets.exposed",
  ]) {
    expect(all).toContain(path);
  }
});
