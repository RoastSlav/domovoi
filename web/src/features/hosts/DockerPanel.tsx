import { useHostDocker, useSetHostDocker } from "../../api/hooks/settings";
import type { HostDocker } from "../../api/types";
import { LoadError, useActionFeedback } from "../../components";
import styles from "./hosts.module.css";

type Choice = "" | "on" | "off";

function choiceOf(state: HostDocker): Choice {
  return state.choice == null ? "" : state.choice ? "on" : "off";
}

/** One sentence on what the agent found and whether the container checks are running. */
function describe(state: HostDocker): string {
  if (!state.connected) {
    return "The agent is offline, so its container runtime is not known yet.";
  }
  if (!state.reported) {
    return "This agent is too old to report its container runtime. Update it to manage Docker from here.";
  }
  const engine = state.engine || "A container engine";
  if (state.locked) {
    return `Fixed in agent.toml: ${state.enabled ? "on" : "off"}. Change \`[docker] enabled\` there to manage it from here.`;
  }
  if (!state.available) {
    return `No container engine can be used: ${state.problem ?? "none was found"}.`;
  }
  return `${engine} found at ${state.endpoint}. The container checks and tools are ${state.enabled ? "on" : "off"}.`;
}

/**
 * The host's container runtime (Docker, Podman or a compatible engine): what the agent
 * found and a switch to turn the container checks and tools on or off. The choice goes to
 * the agent at once, or when it reconnects.
 */
export function DockerPanel({ host }: { host: string }) {
  const docker = useHostDocker(host);
  const save = useActionFeedback(useSetHostDocker(host), {
    success: (saved) =>
      saved.delivered
        ? "Container setting saved and sent to the agent"
        : {
            title: "Container setting saved",
            detail: "The agent is offline. It applies when it reconnects.",
            tone: "info",
          },
    error: "save the container setting",
  });

  if (docker.isPending) {
    return null;
  }
  if (docker.error) {
    return (
      <LoadError
        error={docker.error}
        context="load the container runtime"
        onRetry={docker.refetch}
      />
    );
  }
  const state = docker.data;
  const fixed = state.reported && state.locked;
  const onChange = (value: Choice) => save.mutate(value === "" ? null : value === "on");
  return (
    <section className={styles.form} aria-label="Container runtime">
      <h3>Container runtime</h3>
      <p>{describe(state)}</p>
      <label>
        Containers
        <select
          value={choiceOf(state)}
          disabled={fixed || save.isPending}
          onChange={(e) => onChange(e.target.value as Choice)}
        >
          <option value="">Automatic (use it when found)</option>
          <option value="on" disabled={state.reported && !state.available}>
            Always on
          </option>
          <option value="off">Off</option>
        </select>
      </label>
    </section>
  );
}
