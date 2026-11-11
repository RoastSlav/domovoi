import {
  useTestConnection,
  type Changes,
  type ProviderSlot,
  type TestTarget,
} from "../../api/hooks/settings";
import { Button, InlineMessage, Stack } from "../../components";
import { TestIssues } from "./TestIssues";

const WORDS: Record<TestTarget, { button: string; worked: string; fix: string }> = {
  discord: {
    button: "Send a test message",
    worked: "Discord took a test message",
    fix: "Check the bot token and the channel ID, and that the bot may post in that channel.",
  },
  llm: {
    button: "Test the model",
    worked: "The model answered",
    fix: "Check the provider, the model name and the key, then try again.",
  },
  heartbeat: {
    button: "Ping the heartbeat URL",
    worked: "The heartbeat URL answered",
    fix: "Check the address, and that the hub can reach it.",
  },
};

/**
 * A button that tries a connection with the values on screen (nothing is saved) and says
 * what it checked or why it failed and what to try.
 */
export function SectionTest({
  target,
  slot,
  changes,
}: {
  target: TestTarget;
  slot?: ProviderSlot | undefined;
  /** The candidate values: what a save would send now. */
  changes: () => Changes;
}) {
  const tester = useTestConnection(target, slot ?? "primary");
  const words = WORDS[target];
  const failed = tester.data && !tester.data.ok;
  return (
    <Stack gap={3}>
      <div>
        <Button loading={tester.isPending} onClick={() => tester.mutate(changes())}>
          {slot === "secondary" ? "Test the fallback model" : words.button}
        </Button>
      </div>
      {tester.data?.ok && (
        <InlineMessage tone="success" title={words.worked}>
          <p>{`${tester.data.message} (${tester.data.latency_ms} ms)`}</p>
        </InlineMessage>
      )}
      {failed && (
        <InlineMessage tone="danger" title="The test failed">
          <p>{tester.data?.message}</p>
          <p>{words.fix}</p>
        </InlineMessage>
      )}
      <TestIssues error={tester.error} fix={words.fix} />
    </Stack>
  );
}
