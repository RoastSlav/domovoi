import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import type { Host } from "../../api/types";
import { Button, Cluster, Menu, useToast } from "../../components";
import { describeError } from "../../design/messages";
import { HowToUpdateDialog } from "../updates/HowToUpdateDialog";
import { EnrollDialog } from "./EnrollDialog";
import { RevokeHostDialog } from "./RevokeHostDialog";

/**
 * The actions on one host's page, in its header: the safe ones are buttons (copy the name,
 * how to update an old agent, enroll an agent where there is none); the destructive one,
 * revoking the agent, is in a menu and asks first.
 *
 * Once the agent is revoked the person goes back to the host list.
 */
export function HostActions({ host }: { host: Host }) {
  const navigate = useNavigate();
  const toast = useToast();
  const [dialog, setDialog] = useState<"enroll" | "revoke" | "update">();
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

  return (
    <Cluster gap={2}>
      {host.outdated && (
        <Button icon="refresh" onClick={() => setDialog("update")}>
          How to update
        </Button>
      )}
      {!host.agent_enrolled && !host.connected && host.kind !== "external" && (
        <Button icon="plus" onClick={() => setDialog("enroll")}>
          Enroll agent
        </Button>
      )}
      <Button icon="copy" onClick={() => void copyName()}>
        Copy name
      </Button>
      {host.agent_enrolled && (
        <Menu
          label={`Actions for ${host.name}`}
          text="More"
          items={[
            {
              label: "Revoke agent",
              icon: "ban",
              tone: "danger",
              onSelect: () => {
                setAskedToRevoke(true);
                setDialog("revoke");
              },
            },
          ]}
        />
      )}
      {dialog === "revoke" && <RevokeHostDialog host={host} onClose={() => setDialog(undefined)} />}
      {dialog === "update" && <HowToUpdateDialog onClose={() => setDialog(undefined)} />}
      {dialog === "enroll" && (
        <EnrollDialog initialName={host.name} onClose={() => setDialog(undefined)} />
      )}
    </Cluster>
  );
}
