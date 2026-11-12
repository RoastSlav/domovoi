import { useState } from "react";
import {
  Button,
  Cluster,
  ConfirmDialog,
  CopyBlock,
  CopyButton,
  Dialog,
  DocsLink,
  IconButton,
  InfoTip,
  Kbd,
  LinkButton,
  Menu,
  OptionPicker,
  PasswordPrompt,
  StatusPill,
  RelativeTime,
  Stack,
  Text,
  Tooltip,
} from "../components";
import type { ButtonVariant, PickerOption } from "../components";
import { hostConnectionMeta } from "../design/tone";
import { Demo, Group } from "./Demo";

const PICKER_HOSTS = [
  { value: "", label: "All hosts" },
  {
    value: "nas-01",
    label: "nas-01",
    meta: <StatusPill size="sm" meta={hostConnectionMeta("connected")} />,
  },
  { value: "pi", label: "pi", meta: <StatusPill size="sm" meta={hostConnectionMeta("offline")} /> },
  {
    value: "hub",
    label: "hub",
    meta: <StatusPill size="sm" meta={hostConnectionMeta("hub_only")} />,
  },
];
const PICKER_MANY = Array.from({ length: 12 }, (_, index) => ({
  value: `host-${index + 1}`,
  label: `host-${index + 1}`,
}));

function PickerDemo({
  options,
  decorated = false,
}: {
  decorated?: boolean;
  options: PickerOption[];
}) {
  const [value, setValue] = useState(options[0]?.value ?? "");
  return (
    <OptionPicker
      label="Host"
      value={value}
      options={options}
      onChange={setValue}
      {...(decorated
        ? {
            icon: "server" as const,
            caption: "Host",
            valueMeta: (
              <Text size="xs" muted>
                {options.length - 1} hosts
              </Text>
            ),
          }
        : {})}
    />
  );
}

const FIVE_MINUTES_AGO = new Date(Date.now() - 300_000).toISOString();
const VARIANTS: ButtonVariant[] = ["primary", "secondary", "danger", "ghost"];

/** Buttons and hints: every variant and state, tooltips, info tips and copy. */
/** The account menu's theme choice, as a grouped run of exclusive entries. */
const THEME_ITEMS = (["monitor", "sun", "moon"] as const).map((icon, index) => ({
  label: ["System", "Light", "Dark"][index] ?? "",
  icon,
  group: "Theme",
  checked: index === 0,
  onSelect: () => undefined,
}));

export function ControlDemos() {
  return (
    <Group title="Buttons and hints">
      <Demo name="Button: variant × state (default, hover, small, icon, loading, disabled with reason)">
        <Stack gap={2}>
          {VARIANTS.map((variant) => (
            <Cluster key={variant}>
              <Button variant={variant}>{variant}</Button>
              <Button variant={variant} data-hover>
                Hover
              </Button>
              <Button variant={variant} size="sm">
                Small
              </Button>
              <Button variant={variant} icon="refresh">
                With icon
              </Button>
              <Button variant={variant} loading>
                Loading
              </Button>
              <Button variant={variant} disabledReason="The agent is offline. Reconnect it first.">
                Disabled
              </Button>
            </Cluster>
          ))}
          <Cluster>
            <Button hint="Stops reminders; the incident stays listed.">With a hint</Button>
            <LinkButton to="/dev/design" variant="primary" icon="plus">
              LinkButton
            </LinkButton>
          </Cluster>
        </Stack>
      </Demo>
      <Demo name="IconButton">
        <Cluster>
          <IconButton icon="refresh" label="Refresh" />
          <IconButton icon="copy" label="Copy" variant="secondary" />
          <IconButton icon="close" label="Close" size="sm" />
          <IconButton icon="trash" label="Delete" variant="danger" />
          <IconButton icon="refresh" label="Refresh" loading />
          <IconButton icon="trash" label="Delete" disabledReason="Hub-managed notes stay." />
        </Cluster>
      </Demo>
      <Demo name="Tooltip, InfoTip, DocsLink, Kbd">
        <Cluster gap={4}>
          <Tooltip content="Shown on hover after a moment and on focus at once.">
            <Button>Hover or focus me</Button>
          </Tooltip>
          <span>
            Known condition
            <InfoTip title="Known condition" learnMore="concepts/incidents.html#known-conditions">
              Incidents that match are kept in the history but never alert you and are not triaged.
            </InfoTip>
          </span>
          <DocsLink docs="concepts/incidents.html" topic="incidents" />
          <span>
            Press <Kbd>Esc</Kbd> to close
          </span>
          <span>
            Last seen <RelativeTime at={FIVE_MINUTES_AGO} />
          </span>
        </Cluster>
      </Demo>
      <Demo name="CopyButton, CopyBlock">
        <Stack gap={2}>
          <Cluster>
            <code>a1b2c3d4</code>
            <CopyButton text="a1b2c3d4" />
          </Cluster>
          <CopyBlock label="Enrollment command" text="domovoi agent enroll --token one-time" />
        </Stack>
      </Demo>
    </Group>
  );
}

/** Overlays: dialogs, confirmations, the password prompt and menus. */
export function OverlayDemos() {
  const [open, setOpen] = useState<"dialog" | "confirm" | "typed" | "password" | null>(null);
  const close = () => setOpen(null);
  return (
    <Group title="Overlays">
      <Demo name="OptionPicker: options with a status; more than eight can be filtered by typing">
        <Cluster>
          <PickerDemo options={PICKER_HOSTS} />
          <PickerDemo options={PICKER_MANY} />
          <PickerDemo options={PICKER_HOSTS} decorated />
        </Cluster>
      </Demo>
      <Demo name="Menu: with another icon, a header and a group with one checked choice">
        <Menu
          label="Account"
          icon="user"
          header="Signed in as admin"
          items={[...THEME_ITEMS, { label: "Log out", icon: "logout", onSelect: () => undefined }]}
        />
      </Demo>
      <Demo name="Dialog, ConfirmDialog, PasswordPrompt">
        <Cluster>
          <Button onClick={() => setOpen("dialog")}>Dialog</Button>
          <Button onClick={() => setOpen("confirm")}>ConfirmDialog</Button>
          <Button variant="danger" onClick={() => setOpen("typed")}>
            ConfirmDialog with requireText
          </Button>
          <Button onClick={() => setOpen("password")}>PasswordPrompt</Button>
        </Cluster>
        {open === "dialog" && (
          <Dialog title="A dialog" onClose={close} actions={<Button onClick={close}>Close</Button>}>
            <p>Focus stays inside until it closes; Escape closes it.</p>
            <Tooltip content="Tooltips work inside dialogs.">
              <Button>Hover or focus me</Button>
            </Tooltip>
          </Dialog>
        )}
        {open === "confirm" && (
          <ConfirmDialog
            title="Send the digest now?"
            consequence="It goes to the configured chat now, as well as at its usual time."
            confirmLabel="Send digest"
            onConfirm={close}
            onCancel={close}
          />
        )}
        {open === "typed" && (
          <ConfirmDialog
            title="Revoke agent nas-01?"
            consequence="It disconnects now and needs a new invite to come back."
            confirmLabel="Revoke agent"
            tone="danger"
            requireText="nas-01"
            error={{ title: "Could not revoke the agent", detail: "The hub did not answer." }}
            onConfirm={close}
            onCancel={close}
          />
        )}
        {open === "password" && (
          <PasswordPrompt
            reason="This change affects who can act. Enter your password to save it."
            onConfirm={close}
            onCancel={close}
          />
        )}
      </Demo>
      <Demo name="Menu">
        <Menu
          items={[
            { label: "Edit", icon: "edit", onSelect: () => undefined },
            { label: "Mute for 1 hour", icon: "mute", onSelect: () => undefined },
            {
              label: "Revoke",
              icon: "trash",
              tone: "danger",
              onSelect: () => undefined,
              disabledReason: "the hub's own host",
            },
          ]}
        />
      </Demo>
    </Group>
  );
}
