import { InlineMessage, Stack, StatusPill } from "../../../components";
import { gapInputMeta, gapReasonMeta } from "../../../design/tone";
import { gapsOf } from "../decisionModel";

/**
 * The inputs a decision could not use.
 * @param gaps The stored gap list, as the hub wrote it.
 */
export function GapsBlock({ gaps }: { gaps: unknown }) {
  const list = gapsOf(gaps);
  if (list.length === 0) return null;
  return (
    <Stack gap={2}>
      <InlineMessage tone="warning" title="Decided with missing inputs">
        The result uses the safe reading of each missing input. It is not the whole picture.
      </InlineMessage>
      {list.map((gap) => (
        <div key={gap.input}>
          {gapInputMeta(gap.input).label} <StatusPill meta={gapReasonMeta(gap.reason)} />
        </div>
      ))}
    </Stack>
  );
}
