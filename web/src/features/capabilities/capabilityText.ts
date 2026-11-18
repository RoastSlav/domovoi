import type { Capability } from "../../api/types";

/** The sentence a person reads for one capability state, when the table has no room. */
export function capabilitySentence(capability: Capability): string {
  if (capability.state === "available") {
    return "Implementations the hub would offer here.";
  }
  return capability.describe;
}

/** The catalogue's state in words: a partial list proves nothing about what a host lacks. */
export function catalogueSentence(capability: Capability): string {
  switch (capability.catalogue.state) {
    case "complete":
      return "The host's implementation list was read and verified.";
    case "stale":
      return "The host changed its list after the hub read it, so the rows below are unverified.";
    default:
      return "The host's implementation list was not read completely, so “not installed” cannot be concluded.";
  }
}

/** What a row says about the isolated skill runner, which this milestone never claims works. */
export function runnerSentence(capability: Capability): string {
  if (capability.runner_installed) {
    return "The agent reports a working isolated skill runner.";
  }
  if (capability.root_agent) {
    return "Runs through the control agent today; the isolated skill runner is not installed.";
  }
  return "No isolated skill runner is installed on this host.";
}

/** Where a capability executes, in words rather than a wire value. */
export function executionSentence(execution: string): string {
  switch (execution) {
    case "agent":
      return "Runs on the agent";
    case "skillrunner":
      return "Runs in the skill runner";
    case "helper":
      return "Runs through the helper";
    default:
      return "Runs in the hub";
  }
}
