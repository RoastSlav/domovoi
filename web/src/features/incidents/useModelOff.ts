import { useSettings } from "../../api/hooks/system";

/** Why the model cannot be used now: no model set up, or triage switched off. */
export interface ModelOff {
  /** The line shown where the chat box would be. */
  line: string;
  /** Why "Run triage" cannot be used, and how to enable it. */
  reason: string;
}

/** Where the model and triage are set up. */
export const MODEL_SETTINGS = "/settings/models";

/**
 * Whether the incident page can offer the model at all, known up front from the settings:
 * with no model, or with triage off, there is nothing to ask and nothing to run, so the page
 * shows a line pointing to the settings instead of controls that could only refuse. While
 * the settings load, or fail to, the controls stay and refuse by themselves if need be.
 */
export function useModelOff(): ModelOff | undefined {
  const settings = useSettings().data;
  if (!settings) {
    return undefined;
  }
  if (!settings.llm_primary) {
    return {
      line: "Set up a model to ask about incidents.",
      reason: "No model is set up. Add one under Settings, Models and triage.",
    };
  }
  if (!settings.triage.enabled) {
    return {
      line: "Triage is off, so there is no model to ask.",
      reason: "Triage is off. Turn it on under Settings, Models and triage.",
    };
  }
  return undefined;
}
