import { InlineMessage, Progress } from "../../components";
import { phaseText, type ApplyPhase } from "../settings/useApply";

/**
 * Says where the save is: the hub restarting to apply the settings shows as progress, the
 * end of it as a message, so a person is never left guessing whether the save happened.
 */
export function ApplyStatus({ phase }: { phase: ApplyPhase }) {
  const text = phaseText(phase);
  if (!text) {
    return null;
  }
  if (phase === "applying") {
    return <Progress label={text} />;
  }
  return <InlineMessage tone={phase === "slow" ? "warning" : "success"} title={text} />;
}
