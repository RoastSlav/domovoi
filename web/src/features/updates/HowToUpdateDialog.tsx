import { useUpdate } from "../../api/hooks/updates";
import { Button } from "../../components/Button";
import { CopyBlock } from "../../components/CopyBlock";
import { Dialog } from "../../components/Dialog";

/** The commands that bring a deployment to the release the hub last saw. */
export function HowToUpdateDialog({ onClose }: { onClose: () => void }) {
  const update = useUpdate();
  const info = update.data;
  const latest = info?.latest;
  return (
    <Dialog
      title="How to update Domovoi"
      actions={<Button onClick={onClose}>Close</Button>}
      onClose={onClose}
      wide
    >
      <p>Update the hub first, then the agents.</p>
      {latest ? (
        <>
          <h3>Hub</h3>
          {info.upgrade_command ? (
            <>
              <p>Replace the binary in place:</p>
              <CopyBlock text={info.upgrade_command} label="Hub upgrade command" />
              <p>Or run the installer again:</p>
            </>
          ) : null}
          <CopyBlock text={info.hub_command ?? ""} label="Hub update command" />
          <h3>Each agent</h3>
          <CopyBlock text={info.agent_command ?? ""} label="Agent update command" />
          <h3>Docker</h3>
          <CopyBlock text={info.docker_command ?? ""} label="Docker update command" />
        </>
      ) : (
        <p>No release has been checked yet. Check for one in Settings, under About.</p>
      )}
    </Dialog>
  );
}
