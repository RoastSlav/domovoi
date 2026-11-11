import type { Tone } from "../design/tone";

/**
 * The attributes that give a row or a card a 3 px left stripe in `tone`: spread them on the
 * element, `<li {...toneStripe(meta.tone)}>`. With no tone it adds nothing.
 *
 * Use it to let a list be scanned by status. Never tint the whole row.
 */
export function toneStripe(tone: Tone | undefined): { "data-stripe"?: true; "data-tone"?: Tone } {
  return tone ? { "data-tone": tone, "data-stripe": true } : {};
}
