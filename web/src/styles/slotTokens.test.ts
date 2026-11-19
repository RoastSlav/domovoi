import { SLOT_CONTENT_MIN_PX, SLOT_WIDTH_PX } from "../app/rightSlot/layout";
import tokens from "./tokens.css?raw";

function rem(name: string): number {
  const match = new RegExp(`${name}:\\s*([\\d.]+)rem;`).exec(tokens);
  return Number(match?.[1]) * 16;
}

describe("right slot tokens", () => {
  it("slot_constants_equal_the_tokens", () => {
    expect(SLOT_WIDTH_PX).toBe(rem("--right-slot-width"));
    expect(SLOT_CONTENT_MIN_PX).toBe(rem("--right-slot-content-min"));
  });
});
