import { useUpdate } from "../../api/hooks/updates";
import { Button, CopyBlock, Dialog, InlineMessage, Stack, Text } from "../../components";

/** One way to install: what it is for in a sentence, then the command to paste. */
function Method({
  title,
  note,
  command,
  label,
}: {
  title: string;
  note?: string | undefined;
  command: string | null | undefined;
  label: string;
}) {
  if (!command) {
    return null;
  }
  return (
    <Stack gap={2}>
      <Text strong>{title}</Text>
      {note && (
        <Text muted block>
          {note}
        </Text>
      )}
      <CopyBlock text={command} label={label} />
    </Stack>
  );
}

/** The commands that bring a deployment to the release the hub last saw. */
export function HowToUpdateDialog({ onClose }: { onClose: () => void }) {
  const update = useUpdate();
  const info = update.data;
  return (
    <Dialog
      title="How to update Domovoi"
      actions={<Button onClick={onClose}>Close</Button>}
      onClose={onClose}
      wide
    >
      <Stack gap={4}>
        <Text block>Update the hub first, then the agents.</Text>
        {info?.latest ? (
          <>
            <Method
              title="Hub, replacing the binary"
              note="Replaces the binary in place and restarts the service."
              command={info.upgrade_command}
              label="Hub upgrade command"
            />
            <Method
              title="Hub, running the installer again"
              command={info.hub_command ?? ""}
              label="Hub update command"
            />
            <Method
              title="Each agent"
              note="Run it on every host that has an agent."
              command={info.agent_command ?? ""}
              label="Agent update command"
            />
            <Method
              title="Docker"
              note="For a hub or agent that runs as a container."
              command={info.docker_command ?? ""}
              label="Docker update command"
            />
          </>
        ) : (
          <InlineMessage tone="info" title="No release has been checked yet">
            <p>Check for one in Settings, under About.</p>
          </InlineMessage>
        )}
      </Stack>
    </Dialog>
  );
}
