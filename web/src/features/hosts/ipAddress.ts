/** The 16 bits of each of an IPv6 address's eight groups, or `undefined` when it is not one. */
function groupsOf(address: string): number[] | undefined {
  if (!address.includes(":") || /[^0-9a-f:]/i.test(address) || address.split("::").length > 2) {
    return undefined;
  }
  const [head = "", tail] = address.split("::");
  const side = (text: string) => (text === "" ? [] : text.split(":"));
  const first = side(head);
  const last = tail === undefined ? [] : side(tail);
  const missing = 8 - first.length - last.length;
  if (tail === undefined ? missing !== 0 : missing < 1) {
    return undefined;
  }
  const all = [...first, ...Array<string>(tail === undefined ? 0 : missing).fill("0"), ...last];
  const bits = all.map((group) => (/^[0-9a-f]{1,4}$/i.test(group) ? parseInt(group, 16) : NaN));
  return bits.some(Number.isNaN) ? undefined : bits;
}

/**
 * An address as people should read it: an IPv6 address in the canonical compressed form of
 * RFC 5952 (lower case, no leading zeros, the longest run of zero groups as `::`), anything
 * else exactly as given. Never changes what the address is.
 */
export function displayAddress(address: string): string {
  const groups = groupsOf(address);
  if (!groups) {
    return address;
  }
  let best = { at: -1, length: 0 };
  for (let at = 0; at < groups.length; at += 1) {
    let length = 0;
    while (groups[at + length] === 0) {
      length += 1;
    }
    if (length > best.length) {
      best = { at, length };
    }
    at += Math.max(length - 1, 0);
  }
  const hex = groups.map((group) => group.toString(16));
  if (best.length < 2) {
    return hex.join(":");
  }
  const before = hex.slice(0, best.at).join(":");
  const after = hex.slice(best.at + best.length).join(":");
  return `${before}::${after}`;
}
