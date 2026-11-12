import { Stack } from "../../../components";
import { UpdateAbout } from "../../updates/UpdateAbout";
import styles from "../settings.module.css";
import { AgentVersions } from "./AgentVersions";

/** The hub's version and release check, then the versions its agents run. */
export function AboutSection() {
  return (
    <Stack gap={6}>
      <UpdateAbout />
      <Stack gap={3}>
        <h3 className={styles.subhead}>Agents</h3>
        <AgentVersions />
      </Stack>
    </Stack>
  );
}
