/** The item at `index`, or a clear failure: for tests, where a missing item is a bug in the test. */
export function at<T>(items: readonly T[], index: number): T {
  const item = items[index];
  if (item === undefined) {
    throw new Error(`No item at index ${index}`);
  }
  return item;
}
