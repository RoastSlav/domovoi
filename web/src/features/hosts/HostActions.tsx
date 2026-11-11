import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import type { Host } from "../../api/types";
import { Cluster, Menu, useToast, type MenuItem } from "../../components";
import { describeError } from "../../design/messages";
import { HowToUpdateDialog } from "../updates/HowToUpdateDialog";
import { RevokeHostDialog } from "./RevokeHostDialog";

/**
 * The actions on one host's page, right of its title: a menu with the name copy, how to
 * update an old agent and revoking the agent. Wrapped in a row so a main action can sit
 * beside the menu later without moving it.
 *
 * Revoking asks first; once the agent is revoked the person goes back to the host list.
 */
export function HostActions({ host }: { host: Host }) {
  const navigate = useNavigate();
  const toast = useToast();
  const [dialog, setDialog] = useState<"revoke" | "update">();
  const [askedToRevoke, setAskedToRevoke] = useState(false);

  // The revoke dialog closes after a success and after a cancel alike; the host losing its
  // enrolled agent tells the two apart.
  useEffect(() => {
    if (askedToRevoke && !host.agent_enrolled) {
      void navigate("/hosts");
    }
  }, [askedToRevoke, host.agent_enrolled, navigate]);

  const copyName = async () => {
    try {
      await navigator.clipboard.writeText(host.name);
      toast.success(`Copied ${host.name}`);
    } catch (failure) {
      toast.error(describeError(failure, "copy the host name"));
    }
  };

  const items: MenuItem[] = [{ label: "Copy name", icon: "copy", onSelect: () => void copyName() }];
  if (host.outdated) {
    items.push({ label: "How to update", icon: "refresh", onSelect: () => setDialog("update") });
  }
  items.push({
    label: "Revoke agent",
    icon: "ban",
    tone: "danger",
    disabledReason: host.agent_enrolled ? undefined : "No agent is enrolled on this host",
    onSelect: () => {
      setAskedToRevoke(host.agent_enrolled);
      setDialog("revoke");
    },
  });

  return (
    <Cluster gap={2}>
      <Menu label={`Actions for ${host.name}`} items={items} />
      {dialog === "revoke" && <RevokeHostDialog host={host} onClose={() => setDialog(undefined)} />}
      {dialog === "update" && <HowToUpdateDialog onClose={() => setDialog(undefined)} />}
    </Cluster>
  );
}
