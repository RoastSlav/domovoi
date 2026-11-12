import type { IconName } from "../design/icons";

/** One thing the command palette can do or open. */
export interface PaletteItem {
  /** A muted line at the right: a shortcut, or a short fact such as a host's state. */
  hint?: string;
  icon?: IconName;
  /** Unique within the palette; names the option for the keyboard and assistive technology. */
  id: string;
  /** Extra words that find the item without being shown ("dark" for "Toggle theme"). */
  keywords?: readonly string[];
  /** What the person sees and searches. */
  label: string;
  /** Runs after the palette has closed. */
  onSelect: () => void;
}

/** A titled run of items in the command palette: Pages, Hosts, Actions. */
export interface PaletteGroup {
  items: readonly PaletteItem[];
  title: string;
}

/**
 * The groups with only the items that match `query`: every word of the query must appear
 * somewhere in the item's label or keywords, ignoring case. Groups left empty are dropped.
 * An empty query matches everything.
 */
export function filterGroups(groups: readonly PaletteGroup[], query: string): PaletteGroup[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return groups.filter((group) => group.items.length > 0).map((group) => ({ ...group }));
  }
  return groups
    .map((group) => ({
      title: group.title,
      items: group.items.filter((item) => {
        const haystack = [item.label, ...(item.keywords ?? [])].join(" ").toLowerCase();
        return words.every((word) => haystack.includes(word));
      }),
    }))
    .filter((group) => group.items.length > 0);
}
