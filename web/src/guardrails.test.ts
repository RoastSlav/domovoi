// @vitest-environment node
import { ESLint } from "eslint";

/** Lints `code` as if it lived at `filePath` and returns the rule ids that fired. */
async function rulesFiredBy(code: string, filePath: string): Promise<string[]> {
  const eslint = new ESLint({ cwd: process.cwd() });
  const [result] = await eslint.lintText(code, { filePath });
  return (result?.messages ?? []).map((message) => message.ruleId ?? "parse-error");
}

const PAGE = "src/features/example/ExamplePage.tsx";

describe("lint guardrails for page code", () => {
  it("rejects a native title, a raw button, an inline style and a verdict literal", async () => {
    const code = [
      "export function Bad() {",
      "  return (",
      '    <div title="Hover text" style={{ color: "red" }}>',
      "      <button>Go</button>",
      '      {"ACTION_NEEDED"}',
      "    </div>",
      "  );",
      "}",
    ].join("\n");
    const fired = await rulesFiredBy(code, PAGE);
    expect(fired).toContain("domovoi/no-native-title");
    expect(fired).toContain("domovoi/no-raw-button");
    expect(fired).toContain("no-restricted-syntax");
    expect(fired).toContain("domovoi/no-status-literal");
  });

  it("rejects a disabled control that does not say why", async () => {
    const code = 'export const A = () => <Select disabled>{"x"}</Select>;';
    expect(await rulesFiredBy(code, PAGE)).toContain("domovoi/disabled-needs-reason");
    const reasoned = 'export const A = () => <Checkbox disabled disabledReason="Why" />;';
    expect(await rulesFiredBy(reasoned, PAGE)).not.toContain("domovoi/disabled-needs-reason");
  });

  it("rejects a mutation whose result nothing reports", async () => {
    const code = [
      'import { useAck } from "../../api/hooks/incidents";',
      "export function Silent() {",
      "  const ack = useAck();",
      "  return <Button onClick={() => ack.mutate()}>Ack</Button>;",
      "}",
    ].join("\n");
    expect(await rulesFiredBy(code, PAGE)).toContain("domovoi/mutation-feedback");
  });

  it("accepts a mutation wrapped in useActionFeedback or vouched for in a comment", async () => {
    const wrapped = [
      'import { useActionFeedback } from "../../components";',
      "export function Loud({ ack }) {",
      '  const fb = useActionFeedback(ack, { success: "Done", error: "do it" });',
      "  return <Button onClick={() => fb.mutate()}>Ack</Button>;",
      "}",
    ].join("\n");
    expect(await rulesFiredBy(wrapped, PAGE)).not.toContain("domovoi/mutation-feedback");
    const vouched = [
      "// feedback-ok: the parent shows the error",
      "export const A = ({ m }) => <Button onClick={() => m.mutate()}>Go</Button>;",
    ].join("\n");
    expect(await rulesFiredBy(vouched, PAGE)).not.toContain("domovoi/mutation-feedback");
  });

  it("leaves the kit and tests free to use what pages may not", async () => {
    const code = "export const A = () => <button>Go</button>;";
    expect(await rulesFiredBy(code, "src/components/Example.tsx")).not.toContain(
      "domovoi/no-raw-button",
    );
  });

  it("rejects a page file over the length limit", async () => {
    const code = `${"const a = 1;\n".repeat(260)}export { a };`;
    expect(await rulesFiredBy(code, "src/features/example/Long.ts")).toContain("max-lines");
  });
});
