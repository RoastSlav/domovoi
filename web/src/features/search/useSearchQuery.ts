import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { useDebounced } from "../../components";

/** How long typing must pause before a search is asked and written to the address. */
export const GLOBAL_SEARCH_DEBOUNCE_MS = 300;

/** The longest search text. */
export const MAX_QUERY_CHARS = 200;

/**
 * The search box's text and the settled query behind it. The query follows the text after a
 * pause, is kept in `?q=` (other parameters stay), and clears at once when the text is emptied.
 */
export function useSearchQuery(): { q: string; setText: (text: string) => void; text: string } {
  const [params, setParams] = useSearchParams();
  const fromUrl = (params.get("q") ?? "").slice(0, MAX_QUERY_CHARS);
  const [text, setText] = useState(fromUrl);
  const written = useRef(fromUrl.trim());
  const settled = useDebounced(text, GLOBAL_SEARCH_DEBOUNCE_MS).trim();
  const q = text.trim() === "" ? "" : settled;

  useEffect(() => {
    if (q === written.current) {
      return;
    }
    written.current = q;
    setParams(
      (previous) => {
        const next = new URLSearchParams(previous);
        if (q) {
          next.set("q", q);
        } else {
          next.delete("q");
        }
        return next;
      },
      { replace: true },
    );
  }, [q, setParams]);

  // A link to /search?q=… while the page is open (the palette, a bookmark) fills the box.
  useEffect(() => {
    if (fromUrl.trim() !== written.current) {
      written.current = fromUrl.trim();
      setText(fromUrl);
    }
  }, [fromUrl]);

  return { q, text, setText: (next) => setText(next.slice(0, MAX_QUERY_CHARS)) };
}
