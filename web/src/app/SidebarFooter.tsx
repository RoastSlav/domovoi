import { useNavigate } from "react-router";
import type { LiveStatus } from "../api/events";
import { useSavePreferences } from "../api/hooks/preferences";
import { useLogout, useSession } from "../api/hooks/session";
import { Menu, Tooltip, useActionFeedback, usePresentationChoice } from "../components";
import { Icon, type IconName } from "../design/icons";
import { useTheme, type Theme } from "../design/theme";
import { PROFILE_IDS, PROFILE_META } from "../design/visibility";
import { liveMeta } from "../design/tone";
import styles from "./sidebar.module.css";

const THEMES: readonly (readonly [Theme, string, IconName])[] = [
  ["system", "System", "monitor"],
  ["light", "Light", "sun"],
  ["dark", "Dark", "moon"],
];

/** Whether changes arrive as they happen: a dot and a short word (on the rail, the dot alone). */
function LiveState({ status }: { status: LiveStatus }) {
  const meta = liveMeta(status);
  return (
    <Tooltip content={meta.hint} side="right">
      <div role="status" className={styles.live} data-tone={meta.tone}>
        <span className={styles.liveDot} aria-hidden="true">
          <Icon name="dot" />
        </span>
        <span className={styles.liveLabel}>{meta.label}</span>
      </div>
    </Tooltip>
  );
}

/** Who is signed in, the theme, and the way out. Account settings join this menu later. */
function UserRow() {
  const session = useSession();
  const navigate = useNavigate();
  const logout = useActionFeedback(useLogout(), { success: () => null, error: "sign out" });
  const { theme, setTheme } = useTheme();
  const choice = usePresentationChoice();
  const savePreferences = useActionFeedback(useSavePreferences(), {
    success: (_saved, variables) => variables.message ?? null,
    error: "save the detail level",
  });
  const username = session.data?.user.username ?? "";
  const onLogout = () =>
    logout.mutate(undefined, { onSuccess: () => void navigate("/login", { replace: true }) });
  return (
    <div className={styles.user}>
      <span className={styles.avatar} aria-hidden="true">
        {username.slice(0, 1).toUpperCase()}
      </span>
      <span className={styles.username}>{username}</span>
      <Menu
        label="Account menu"
        icon="more"
        header={`Signed in as ${username}`}
        items={[
          ...THEMES.map(([value, label, icon]) => ({
            label,
            icon,
            group: "Theme",
            checked: theme === value,
            onSelect: () => setTheme(value),
          })),
          ...PROFILE_IDS.map((profile) => ({
            label: PROFILE_META[profile].label,
            group: "Detail level",
            checked: choice.profile === profile,
            onSelect: () =>
              savePreferences.mutate({
                presentation: { profile, pages: { ...choice.pages } },
                message: `Detail level set to ${PROFILE_META[profile].label}.`,
              }),
          })),
          {
            label: "Display settings",
            icon: "gear" as const,
            onSelect: () => void navigate("/settings/display"),
          },
          { label: "Log out", icon: "logout", onSelect: onLogout },
        ]}
      />
    </div>
  );
}

/** The bottom of the sidebar: the live state, then the signed-in user. */
export function SidebarFooter({ live }: { live: LiveStatus }) {
  return (
    <div className={styles.footer}>
      <LiveState status={live} />
      <UserRow />
    </div>
  );
}
