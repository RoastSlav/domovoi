import axe from "axe-core";

/**
 * Runs axe over `container` and fails with a readable list when it finds a violation.
 *
 * Colour contrast is left out: jsdom has no layout or computed colours, so that check
 * cannot work here. `styles/contrast.test.ts` proves every token pair instead.
 */
export async function expectNoA11yViolations(container: Element): Promise<void> {
  const results = await axe.run(container, {
    rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
  });
  const found = results.violations.map(
    (violation) =>
      `${violation.id}: ${violation.help}\n` +
      violation.nodes.map((node) => `    ${node.html.slice(0, 160)}`).join("\n"),
  );
  expect(found, found.join("\n")).toEqual([]);
}

let skipped = false;

/**
 * Opts the running test out of the automatic accessibility check in `setup.ts`. Call it
 * only for a test that renders something deliberately broken or a catalogue of fragments,
 * and say why in a comment next to the call.
 */
export function skipAxe(): void {
  skipped = true;
}

/** Whether the running test opted out; reading it resets it for the next test. */
export function takeSkip(): boolean {
  const value = skipped;
  skipped = false;
  return value;
}
