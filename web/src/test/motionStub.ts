/** The layout duration a test wants `motionMs` to read; 0 means reduced motion. */
export const motionStub = { ms: 240 };

/**
 * For `vi.mock("<path>/design/motion", async (original) => stubbedMotion(await original()))`:
 * keeps the module and makes `motionMs` read `motionStub.ms`, whichever token is asked for.
 */
export function stubbedMotion<T extends object>(actual: T): T {
  return { ...actual, motionMs: () => motionStub.ms };
}
