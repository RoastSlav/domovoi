import { Component, lazy, Suspense, useState, type ReactNode } from "react";
import { Button } from "../Button";
import { InlineMessage } from "../InlineMessage";
import { Skeleton } from "../Skeleton";
import styles from "./graphSkeleton.module.css";
import type { StepGraphProps } from "./types";

const loadEngine = () => import("./GraphEngine");
// The shell is library-free but only needed with a graph, so it travels with the lazy chunks.
const loadShell = () => import("./GraphShell").then((m) => ({ default: m.GraphShell }));
const fresh = () => ({ Engine: lazy(loadEngine), Shell: lazy(loadShell) });

/** Starts the engine and shell downloads; safe to call repeatedly (each module loads once). */
export function preloadGraph(): Promise<void> {
  return Promise.all([loadEngine(), loadShell()]).then(
    () => undefined,
    () => undefined,
  );
}

/** Catches a rejected engine import (or a throw while drawing) and tells the parent. */
class EngineBoundary extends Component<
  { children: ReactNode; onError(): void },
  { failed: boolean }
> {
  override state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  override componentDidCatch(): void {
    this.props.onError();
  }

  override render(): ReactNode {
    return this.state.failed ? null : this.props.children;
  }
}

/**
 * The interactive graph of a step trace: a library-free shell (focus, keyboard, announcements,
 * toolbar, edge key) around an engine that loads as its own chunk the first time a graph is
 * shown. If the chunk does not load or the engine throws, the graph is replaced by a message
 * with "Try again"; the page around it keeps working.
 *
 * Use it for one trace's steps and their edges. Do not use it for a diagram that is not a
 * step graph, and never feed it HTML: every word is drawn as text.
 */
export function StepGraph(props: StepGraphProps) {
  // A fresh `lazy()` per attempt, so a rejected import is requested again.
  const [state, setState] = useState(() => ({ attempt: 0, failures: 0, ...fresh() }));
  const { Engine, Shell } = state;
  const failed = state.failures > state.attempt;
  const fail = (): void =>
    setState((s) => (s.failures > s.attempt ? s : { ...s, failures: s.attempt + 1 }));
  if (failed) {
    return (
      <InlineMessage
        tone="warning"
        title="The graph could not be drawn"
        action={
          <Button
            size="sm"
            onClick={() => setState((s) => ({ ...s, attempt: s.attempt + 1, ...fresh() }))}
          >
            Try again
          </Button>
        }
      >
        The step list below has the same steps.
        {state.attempt >= 1 && " If it keeps failing, reload the page."}
      </InlineMessage>
    );
  }
  return (
    <EngineBoundary key={state.attempt} onError={fail}>
      <Suspense
        fallback={
          <div className={styles.skeleton}>
            <Skeleton shape="block" />
          </div>
        }
      >
        <Shell {...props} Engine={Engine} onFailure={fail} />
      </Suspense>
    </EngineBoundary>
  );
}
