import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { RightSlotHost, RightSlotProvider } from "../app/rightSlot";

/** Width of the page body the next `renderSlot` measures; set by `stubSlotEnvironment`. */
let bodyWidth = 1300;

/**
 * Makes jsdom answer as a screen of some width would: the media query for a wide screen and a
 * `ResizeObserver` that reports `width` for the page body. Call it before rendering.
 */
export function stubSlotEnvironment({ width, wide = true }: { width: number; wide?: boolean }) {
  bodyWidth = width;
  vi.stubGlobal("matchMedia", () => ({
    matches: wide,
    addEventListener() {},
    removeEventListener() {},
  }));
  vi.stubGlobal(
    "ResizeObserver",
    class {
      private readonly report: ResizeObserverCallback;
      constructor(callback: ResizeObserverCallback) {
        this.report = callback;
      }
      observe() {
        this.report([{ contentRect: { width: bodyWidth } } as ResizeObserverEntry], this as never);
      }
      unobserve() {}
      disconnect() {}
    },
  );
}

/** Renders `children` in the shell's frame: provider, page body, `<main>` and the slot host. */
export function renderSlot(children: ReactNode, environment = { width: 1300, wide: true }) {
  stubSlotEnvironment(environment);
  return render(
    <RightSlotProvider>
      <div data-testid="body">
        <main id="main" tabIndex={-1} data-slot-inert>
          {children}
        </main>
        <RightSlotHost />
      </div>
    </RightSlotProvider>,
  );
}
