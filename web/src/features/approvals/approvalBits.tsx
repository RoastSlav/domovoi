import { Countdown, Tag, useRemaining } from "../../components";

/** Under this much time left, the countdown of a waiting action turns to a warning. */
export const EXPIRY_WARNING_MS = 15 * 60 * 1000;

/** Who suggested the action, as a sentence. The hub sends `system:triage` or `user:<id>`. */
export function suggestedBy(proposedBy: string): string {
  switch (proposedBy) {
    case "system:triage":
      return "Suggested by triage";
    case "system:chat":
      return "Suggested in chat";
    default:
      return proposedBy.startsWith("user:") ? "Suggested by a person" : "Suggested by Domovoi";
  }
}

/** The time left to decide, as a tag that turns to a warning near the end. */
export function Expiry({ until }: { until: string }) {
  const remaining = useRemaining(until);
  if (remaining === 0) {
    return <Tag>Expired</Tag>;
  }
  return (
    <Tag {...(remaining < EXPIRY_WARNING_MS ? { tone: "warning" as const } : {})}>
      Expires in <Countdown remaining={remaining} until={until} />
    </Tag>
  );
}
