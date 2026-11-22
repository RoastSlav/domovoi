/** Runs one animation at a time on `requestAnimationFrame`; a new `run` cancels the last. */
export interface Animator {
  /**
   * Calls `frame` with progress 0..1 over `ms` milliseconds, eased out. A duration of 0 (or
   * less) calls `frame(1)` at once and schedules nothing.
   */
  run(ms: number, frame: (progress: number) => void): void;
  cancel(): void;
}

/** The ease-out curve of the tweens: fast start, soft landing. */
export function easeOut(progress: number): number {
  return 1 - (1 - progress) ** 3;
}

/** Makes an `Animator`. */
export function createAnimator(): Animator {
  let handle = 0;
  const cancel = (): void => {
    if (handle !== 0) {
      cancelAnimationFrame(handle);
      handle = 0;
    }
  };
  return {
    cancel,
    run(ms, frame) {
      cancel();
      if (ms <= 0 || typeof requestAnimationFrame !== "function") {
        frame(1);
        return;
      }
      const start = performance.now();
      const tick = (now: number): void => {
        const progress = Math.min(1, (now - start) / ms);
        frame(progress);
        handle = progress < 1 ? requestAnimationFrame(tick) : 0;
      };
      frame(0);
      handle = requestAnimationFrame(tick);
    },
  };
}
