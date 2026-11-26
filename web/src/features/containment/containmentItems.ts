import type { ContainmentStatus } from "../../api/types";
import type { AttentionItem } from "../attention/attentionModel";
import { LOST_WHY, gateAttentionWhy } from "../../design/containmentText";
import { blockPhaseMeta, gateStateMeta } from "../../design/tone";

/** One host's containment status, with the host it belongs to. */
export interface ContainmentAttentionInput {
  hostId: string;
  hostName: string;
  status: ContainmentStatus;
}

const HELPER_TAB = "/capabilities?tab=helper";

function rank(base: number, fields: Omit<AttentionItem, "rank" | "tier">): AttentionItem {
  return { ...fields, rank: base, tier: base <= 4 ? 0 : base <= 8 ? 1 : 2 };
}

/** Whether a block was lost on this host, as the blocks or the helper's last answer say. */
function lostOn(status: ContainmentStatus): boolean {
  return status.blocks.some((block) => block.phase === "lost") || Boolean(status.lost_blocks);
}

/**
 * The attention items for blocks: a lost block is act-now (the source may no longer be
 * blocked); a gate that refuses to block is worth a look. A host whose gate was never asked
 * ("unknown") or is open has none.
 */
export function containmentItems(inputs: readonly ContainmentAttentionInput[]): AttentionItem[] {
  return inputs.flatMap(({ hostId, hostName, status }) => {
    const items: AttentionItem[] = [];
    const since = status.observed_at ?? null;
    if (lostOn(status)) {
      items.push(
        rank(1, {
          key: `containment:lost:${hostId}`,
          kind: "containment",
          source: "containment",
          title: `Blocks lost on ${hostName}`,
          meta: blockPhaseMeta("lost"),
          hostIds: [hostId],
          href: HELPER_TAB,
          since,
          expiresAt: null,
          why: LOST_WHY,
        }),
      );
    }
    const state = status.gate.state;
    if (state === "blocked_leak" || state === "blocked_untested") {
      items.push(
        rank(9, {
          key: `containment:gate:${hostId}`,
          kind: "containment",
          source: "containment",
          title: `Blocking is off on ${hostName}`,
          meta: gateStateMeta(state),
          hostIds: [hostId],
          href: HELPER_TAB,
          since,
          expiresAt: null,
          why: gateAttentionWhy(state),
        }),
      );
    }
    return items;
  });
}
