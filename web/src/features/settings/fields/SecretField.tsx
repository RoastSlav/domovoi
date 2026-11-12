import { useState } from "react";
import type { SecretChange } from "../../../api/hooks/settings";
import { Button, Cluster, Tag, TextInput, fieldIdForPath } from "../../../components";
import { SettingRow } from "../SettingRow";
import { errorText, type Common } from "./common";
import { SourceTag } from "./SourceTag";

function stateOf(isSet: boolean, change: SecretChange): { label: string; tone?: "warning" } {
  if ("clear" in change) {
    return { label: "Will be removed", tone: "warning" };
  }
  if ("set" in change && change.set.length > 0) {
    return { label: "Will be set", tone: "warning" };
  }
  return { label: isSet ? "Set" : "Not set" };
}

/**
 * A write-only saved secret. Pending replacements stay local until the settings are saved;
 * Remove clears the saved value. Left alone, it is kept. The value is never shown.
 */
export function SecretField({
  spec,
  isSet,
  change,
  onChange,
  hint,
  errors,
  locked,
  source,
}: Common & {
  isSet: boolean;
  change: SecretChange;
  onChange: (change: SecretChange) => void;
}) {
  const hasDraft = "set" in change && change.set.length > 0;
  const [replacing, setReplacing] = useState(hasDraft);
  const state = stateOf(isSet, change);
  const error = errorText(errors);
  const tags = (
    <>
      <Tag {...(state.tone ? { tone: state.tone } : {})}>{state.label}</Tag>
      {source && <SourceTag source={source} />}
    </>
  );
  if (replacing && !locked) {
    return (
      <SettingRow label={spec.label} path={spec.path} hint={hint} error={error} badges={tags} wide>
        {(wiring) => (
          <Cluster>
            <TextInput
              {...wiring}
              type="password"
              autoComplete="new-password"
              value={"set" in change ? change.set : ""}
              onChange={(event) =>
                onChange(event.target.value ? { set: event.target.value } : { keep: true })
              }
            />
            <Button
              variant="primary"
              disabledReason={hasDraft ? undefined : "Type the new value first."}
              onClick={() => setReplacing(false)}
            >
              Set
            </Button>
            <Button
              onClick={() => {
                setReplacing(false);
                onChange({ keep: true });
              }}
            >
              Cancel
            </Button>
          </Cluster>
        )}
      </SettingRow>
    );
  }
  const cleared = "clear" in change;
  const verb = hasDraft ? "Edit" : isSet ? "Replace" : "Set";
  return (
    <SettingRow
      label={spec.label}
      path={spec.path}
      hint={hint}
      error={error}
      locked={locked}
      badges={tags}
      plainLabel
    >
      {() =>
        !locked && (
          <Cluster justify="end">
            {cleared ? (
              <Button id={fieldIdForPath(spec.path)} onClick={() => onChange({ keep: true })}>
                Undo
              </Button>
            ) : (
              <>
                <Button id={fieldIdForPath(spec.path)} onClick={() => setReplacing(true)}>
                  {`${verb} ${spec.label}`}
                </Button>
                {isSet && (
                  <Button variant="ghost" onClick={() => onChange({ clear: true })}>
                    {`Remove ${spec.label}`}
                  </Button>
                )}
              </>
            )}
          </Cluster>
        )
      }
    </SettingRow>
  );
}
