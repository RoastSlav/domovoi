import { useNavigate } from "react-router";
import type { IncidentSummary } from "../../api/types";
import { Menu, type MenuItem } from "../../components";
import { MUTE_OPTIONS, useIncidentActions } from "./useIncidentActions";

/**
 * The quick actions of one incident in a list, behind a "More" button: acknowledge, mute
 * for a while or unmute, and open. Each reports its result as a toast.
 */
export function IncidentRowMenu({ incident }: { incident: IncidentSummary }) {
  const navigate = useNavigate();
  const { ack, mute, unmute } = useIncidentActions(incident.id);
  const live = incident.status !== "resolved";

  const items: MenuItem[] = [];
  if (live && incident.status !== "acked") {
    items.push({ label: "Acknowledge", icon: "check", onSelect: () => ack.mutate() });
  }
  if (incident.status === "muted") {
    items.push({ label: "Unmute", icon: "play", onSelect: () => unmute.mutate() });
  } else if (live) {
    for (const [minutes, label] of MUTE_OPTIONS) {
      items.push({
        label: `Mute for ${label}`,
        icon: "mute",
        onSelect: () => mute.mutate(minutes),
      });
    }
  }
  items.push({
    label: "Open",
    icon: "chevron-right",
    onSelect: () => void navigate(`/incidents/${incident.id}`),
  });

  return <Menu label={`Actions for ${incident.title}`} items={items} />;
}
