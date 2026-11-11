import { Tag } from "../../../components";

const PRECEDENCE =
  "A value in the environment or in hub.toml wins over one set here, so those fields are read-only. A value nobody set uses the default.";

/** The words and the explanation for where a setting's value comes from. */
export function describeSource(source: string): { label: string; hint: string } {
  if (source === "file") {
    return { label: "from hub.toml", hint: `Written in hub.toml. ${PRECEDENCE}` };
  }
  if (source.startsWith("env:")) {
    return {
      label: "from environment",
      hint: `Set by the ${source.slice(4)} environment variable. ${PRECEDENCE}`,
    };
  }
  if (source === "ui") {
    return { label: "set here", hint: `Saved from this page. ${PRECEDENCE}` };
  }
  return {
    label: "default",
    hint: `Nobody set this, so the built-in value is used. ${PRECEDENCE}`,
  };
}

/** A tag saying where a setting's value comes from, with the precedence rules on hover. */
export function SourceTag({ source }: { source: string }) {
  const { label, hint } = describeSource(source);
  return <Tag hint={hint}>{label}</Tag>;
}
