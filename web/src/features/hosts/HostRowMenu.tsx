import { useState } from "react";
import { useNavigate } from "react-router";
import type { Host } from "../../api/types";
import { Menu, useToast, type MenuItem } from "../../components";
import { describeError } from "../../design/messages";
import { HowToUpdateDialog } from "../updates/HowToUpdateDialog";
import { RevokeHostDialog } from "./RevokeHostDialog";

/** The URL of a host's page; the name may hold characters that need encoding. */
export function hostPath(name: string): string {
  return `/hosts/${encodeURIComponent(name)}`;
}

/**
 * The actions of one host in the list, behind a "More" button: open it, copy its name, see
 * how to update an old agent, and revoke the agent (which asks first).
 */
export function HostRowMenu({ host }: { host: Host }) {
  const navigate = useNavigate();
  const toast = useToast();
  const [dialog, setDialog] = useState<"revoke" | "update">();

  const copyName = async () => {
    try {
      await navigator.clipboard.writeText(host.name);
      toast.success(`Copied ${host.name}`);
    } catch (failure) {
      toast.error(describeError(failure, "copy the host name"));
    }
  };

  const items: MenuItem[] = [
    { label: "Open", icon: "chevron-right", onSelect: () => void navigate(hostPath(host.name)) },
    { label: "Copy name", icon: "copy", onSelect: () => void copyName() },
  ];
  if (host.outdated) {
    items.push({ label: "How to update", icon: "refresh", onSelect: () => setDialog("update") });
  }
  items.push({
    label: "Revoke agent",
    icon: "ban",
    tone: "danger",
    disabledReason: host.agent_enrolled ? undefined : "No agent is enrolled on this host",
    onSelect: () => setDialog("revoke"),
  });

  return (
    <>
      <Menu label={`Actions for ${host.name}`} items={items} />
      {dialog === "revoke" && <RevokeHostDialog host={host} onClose={() => setDialog(undefined)} />}
      {dialog === "update" && <HowToUpdateDialog onClose={() => setDialog(undefined)} />}
    </>
  );
}
