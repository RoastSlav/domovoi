import { Stack, Text } from "../../../components";
import { UpdateAbout } from "../../updates/UpdateAbout";
import { AgentVersions } from "./AgentVersions";

/** The hub's version and release check, then the versions its agents run. */
export function AboutSection() {
  return (
    <Stack gap={5}>
      <UpdateAbout />
      <Stack gap={2}>
        <Text strong>Agents</Text>
        <AgentVersions />
      </Stack>
    </Stack>
  );
}
