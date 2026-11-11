import { useState } from "react";
import type { SecretChange } from "../api/hooks/settings";
import {
  AuthCard,
  Button,
  DropZone,
  Field as KitField,
  Highlight,
  PasswordInput,
  Stack,
  Stepper,
  Text,
  TextInput,
} from "../components";
import { Field, SecretField } from "../features/settings/fields";
import type { FieldSpec } from "../features/settings/form";
import { Demo, Group } from "./Demo";

const text = (path: string, label: string): FieldSpec => ({ path, label, kind: "text" });

/** One settings field per state: each source, locked by file and environment, with an issue. */
function SettingStates() {
  const [name, setName] = useState<string | boolean>("homelab");
  const [quiet, setQuiet] = useState<string | boolean>(true);
  const ignore = () => undefined;
  return (
    <Stack gap={4}>
      <Field
        spec={text("demo.name", "Set here")}
        value={name}
        onChange={setName}
        hint="A value saved from the Settings page."
        source="ui"
      />
      <Field
        spec={text("demo.default", "Default")}
        value="03:00"
        onChange={ignore}
        hint="Nobody set it, so the built-in value shows."
        source="default"
      />
      <Field
        spec={text("demo.file", "Locked by hub.toml")}
        value="https://hub.example.net"
        onChange={ignore}
        source="file"
        locked="Set in hub.toml; edit the file to change it."
      />
      <Field
        spec={text("demo.env", "Locked by the environment")}
        value="8080"
        onChange={ignore}
        source="env:DOMOVOI_WEB__PORT"
        locked="Set by DOMOVOI_WEB__PORT; change the environment to change it."
      />
      <Field
        spec={text("demo.port", "With an issue from the hub")}
        value="70000"
        onChange={ignore}
        source="ui"
        errors={["Port must be between 1 and 65535."]}
      />
      <Field
        spec={{ path: "demo.quiet", label: "A switch", kind: "bool" }}
        value={quiet}
        onChange={setQuiet}
        hint="On or off, saved with the rest."
        source="ui"
      />
    </Stack>
  );
}

/** A secret that is set, one that is not, and one fixed in the environment. */
function SecretStates() {
  const [token, setToken] = useState<SecretChange>({ keep: true });
  const [key, setKey] = useState<SecretChange>({ keep: true });
  const secret = (path: string, label: string): FieldSpec => ({ path, label, kind: "secret" });
  return (
    <Stack gap={4}>
      <SecretField
        spec={secret("demo.token", "A secret that is set")}
        isSet
        change={token}
        onChange={setToken}
        hint="Replace reveals an empty input. The saved value is never shown."
        source="ui"
      />
      <SecretField
        spec={secret("demo.key", "A secret that is not set")}
        isSet={false}
        change={key}
        onChange={setKey}
        source="default"
      />
      <SecretField
        spec={secret("demo.env_key", "A secret locked by the environment")}
        isSet
        change={{ keep: true }}
        onChange={() => undefined}
        source="env:DOMOVOI_LLM__PRIMARY__API_KEY"
        locked="Set by DOMOVOI_LLM__PRIMARY__API_KEY; change the environment to change it."
      />
    </Stack>
  );
}

/** Settings, first run and sign-in: field states, the stepper, uploads, search marks, the auth card. */
export function SystemDemos() {
  const [password, setPassword] = useState("");
  const [dropped, setDropped] = useState<string>("Nothing chosen yet.");
  return (
    <Group title="Settings, setup and sign-in">
      <Demo name="Settings field: sources, locked, issue">
        <SettingStates />
      </Demo>
      <Demo name="Settings secret: set, not set, locked">
        <SecretStates />
      </Demo>
      <Demo name="Stepper">
        <Stepper
          label="Example steps"
          steps={[
            { id: "hub", label: "Hub", state: "done" },
            { id: "discord", label: "Discord", state: "skipped" },
            { id: "model", label: "Model", state: "current" },
            { id: "host", label: "First host", state: "upcoming" },
          ]}
        />
      </Demo>
      <Demo name="PasswordInput">
        <KitField label="Password" hint="Press Caps Lock while typing to see the note.">
          <PasswordInput
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            autoComplete="off"
          />
        </KitField>
      </Demo>
      <Demo name="DropZone">
        <Stack gap={2}>
          <DropZone
            label="Drop Markdown files here, or choose them"
            hint="Each .md file becomes one note."
            accept=".md"
            folderLabel="Choose a folder instead"
            onFiles={(files) => setDropped(`${files.length} chosen.`)}
          />
          <Text muted>{dropped}</Text>
          <DropZone
            label="Drop files here"
            disabledReason="Choose a host first."
            onFiles={() => undefined}
          />
        </Stack>
      </Demo>
      <Demo name="Highlight">
        <Text>
          <Highlight
            text="Restart the nginx container after a config change"
            query="nginx config"
          />
        </Text>
      </Demo>
      <Demo name="AuthCard">
        <AuthCard title="Log in to Domovoi" intro="The frame of sign-in and account setup.">
          <KitField label="Username">
            <TextInput defaultValue="admin" />
          </KitField>
          <Button variant="primary">Log in</Button>
        </AuthCard>
      </Demo>
    </Group>
  );
}
