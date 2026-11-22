import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import type { Playback } from "./usePlayback";

/** Marks the graph's frame while a past moment is drawn, without touching the graph's own code. */
function useReplayMark(wrapper: { current: HTMLElement | null }, replaying: boolean): void {
  useEffect(() => {
    const element = wrapper.current;
    if (!element) {
      return undefined;
    }
    const apply = (): void => {
      const frame = element.querySelector("[data-graph-frame]");
      if (frame && frame.hasAttribute("data-replay") !== replaying) {
        if (replaying) {
          frame.setAttribute("data-replay", "true");
        } else {
          frame.removeAttribute("data-replay");
        }
      }
    };
    apply();
    const observer = new MutationObserver(apply);
    observer.observe(element, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [wrapper, replaying]);
}

/**
 * Wraps the graph. While the replay bar is open, `,` `.` Space and `l` pressed on the graph's
 * root move the replay (previous, next, play or pause, live); the graph's own keys are not
 * touched, and with the bar closed nothing here reacts. Also marks the frame while replaying.
 */
export function ReplayFrame({
  playback,
  open,
  replaying,
  children,
}: {
  children: ReactNode;
  open: boolean;
  playback: Playback;
  replaying: boolean;
}) {
  const wrapper = useRef<HTMLDivElement>(null);
  useReplayMark(wrapper, replaying);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const target = event.target;
    if (
      !open ||
      event.defaultPrevented ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey ||
      event.shiftKey ||
      !(target instanceof HTMLElement) ||
      !target.hasAttribute("data-graph")
    ) {
      return;
    }
    const actions: Record<string, () => void> = {
      ",": playback.previous,
      ".": playback.next,
      " ": playback.state.playing ? playback.pause : playback.play,
      l: playback.live,
    };
    const act = actions[event.key];
    if (act) {
      event.preventDefault();
      act();
    }
  };
  return (
    // The keys only bubble up from the graph's own focusable root; this element is not a control.
    <div ref={wrapper} onKeyDown={onKeyDown}>
      {children}
    </div>
  );
}
