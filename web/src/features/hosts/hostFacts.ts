/** What a count of open incidents is called: "open incident" for one, "open incidents" otherwise. */
export function openIncidentsNoun(count: number): string {
  return count === 1 ? "open incident" : "open incidents";
}
