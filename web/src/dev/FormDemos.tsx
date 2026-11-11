import { useState } from "react";
import {
  Checkbox,
  Field,
  FormErrorSummary,
  NumberInput,
  SegmentedControl,
  Select,
  Stack,
  SubmitBar,
  Switch,
  TextInput,
  Textarea,
} from "../components";
import { Demo, Group } from "./Demo";

/** Forms: fields with hints and errors, every input, the error summary and the submit bar. */
export function FormDemos() {
  const [mode, setMode] = useState("list");
  const [on, setOn] = useState(true);
  const [checked, setChecked] = useState(false);
  return (
    <Group title="Forms">
      <Demo name="Field with TextInput, NumberInput, Select, Textarea">
        <Stack gap={3}>
          <FormErrorSummary
            issues={[{ path: "web.port", message: "Port must be between 1 and 65535." }]}
          />
          <Field label="Host name" hint="Lower-case letters, digits and dashes." required>
            <TextInput defaultValue="nas-01" mono />
          </Field>
          <Field label="Port" path="web.port" error="Port must be between 1 and 65535.">
            <NumberInput defaultValue={70000} />
          </Field>
          <Field label="Mute for">
            <Select defaultValue="60">
              <option value="60">1 hour</option>
              <option value="1440">1 day</option>
            </Select>
          </Field>
          <Field label="Script" hint="Runs on the host as the agent's user.">
            <Textarea mono defaultValue={"#!/bin/sh\nsystemctl restart nginx.service"} />
          </Field>
          <Field label="Read-only" hint="Set in hub.toml.">
            <TextInput defaultValue="0.0.0.0:7420" disabled />
          </Field>
        </Stack>
      </Demo>
      <Demo name="Checkbox, Switch, SegmentedControl">
        <Stack gap={3}>
          <Checkbox
            label="Include logins from the internet"
            checked={checked}
            onChange={setChecked}
          />
          <Switch label="Enable the check" hint="Runs every 60 s." checked={on} onChange={setOn} />
          <Switch
            label="Container checks"
            checked={false}
            onChange={() => undefined}
            disabledReason="Fixed in agent.toml."
          />
          <SegmentedControl
            label="View"
            value={mode}
            onChange={setMode}
            options={[
              { value: "list", label: "List" },
              { value: "grouped", label: "Grouped" },
            ]}
          />
        </Stack>
      </Demo>
      <Demo name="SubmitBar">
        <SubmitBar changes={3} onDiscard={() => undefined} onSave={() => undefined} />
      </Demo>
    </Group>
  );
}
