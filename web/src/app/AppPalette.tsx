import { useNavigate } from "react-router";
import { useHosts } from "../api/hooks/hosts";
import { useLogout } from "../api/hooks/session";
import { CommandPalette, useActionFeedback, type PaletteGroup } from "../components";
import { useTheme } from "../design/theme";
import { hostConnectionMeta, hostConnectionOf } from "../design/tone";
import { NAV_ITEMS } from "./nav";
import { useIncidentItems, useSettingsItems } from "./paletteSources";

/**
 * The command palette of the signed-in app: jump to any page, host, open incident or settings section, switch the theme,
 * collapse the sidebar or sign out. Ctrl+K (Cmd+K on a Mac) or the sidebar's search opens it.
 */
export function AppPalette({
  open,
  onClose,
  onToggleRail,
}: {
  onClose: () => void;
  onToggleRail: () => void;
  open: boolean;
}) {
  const navigate = useNavigate();
  const hosts = useHosts();
  const { theme, setTheme } = useTheme();
  const logout = useActionFeedback(useLogout(), { success: () => null, error: "sign out" });
  const go = (to: string) => () => void navigate(to);
  const incidents = useIncidentItems(open, go);
  const settings = useSettingsItems(open, go);

  const groups: PaletteGroup[] = [
    {
      title: "Pages",
      items: NAV_ITEMS.map((item) => ({
        id: `page-${item.to}`,
        label: item.label,
        icon: item.icon,
        onSelect: go(item.to),
      })),
    },
    {
      title: "Hosts",
      items: (hosts.data ?? []).map((host) => ({
        id: `host-${host.name}`,
        label: host.name,
        icon: "server",
        hint: hostConnectionMeta(hostConnectionOf(host)).label,
        keywords: ["host"],
        onSelect: go(`/hosts/${encodeURIComponent(host.name)}`),
      })),
    },
    { title: "Incidents", items: incidents },
    { title: "Settings", items: settings },
    {
      title: "Actions",
      items: [
        {
          id: "theme",
          label: theme === "dark" ? "Switch to the light theme" : "Switch to the dark theme",
          icon: theme === "dark" ? "sun" : "moon",
          keywords: ["theme", "dark", "light"],
          onSelect: () => setTheme(theme === "dark" ? "light" : "dark"),
        },
        {
          id: "rail",
          label: "Collapse or expand the sidebar",
          icon: "menu",
          keywords: ["sidebar", "rail"],
          onSelect: onToggleRail,
        },
        {
          id: "logout",
          label: "Log out",
          icon: "logout",
          onSelect: () =>
            logout.mutate(undefined, {
              onSuccess: () => void navigate("/login", { replace: true }),
            }),
        },
      ],
    },
  ];

  return <CommandPalette open={open} onClose={onClose} groups={groups} />;
}
