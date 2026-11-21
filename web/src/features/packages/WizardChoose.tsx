import { usePackageDetail, usePackages } from "../../api/hooks/packages";
import {
  Button,
  InlineMessage,
  QueryBoundary,
  SegmentedControl,
  Select,
  Skeleton,
  Stack,
  Text,
  Visible,
  Field,
} from "../../components";
import styles from "./wizard.module.css";
import { canContinue, type WizardState } from "./wizardState";

const MISSING = "The package file is missing from the store. Import it again.";

/**
 * Step one: install or remove, and which package. A package whose file is gone from the
 * store is flagged and blocks Next. Not for choosing hosts.
 */
export function WizardChoose({
  state,
  set,
  fixed,
  blocked,
  onNext,
}: {
  /** The package came from the address and a removal names it: shown read-only. */
  fixed: boolean;
  /** A reason the whole wizard cannot go on, such as installation being off. */
  blocked: string | undefined;
  onNext: () => void;
  set: (patch: Partial<WizardState>) => void;
  state: WizardState;
}) {
  const packages = usePackages();
  const installing = state.kind === "install";
  const detail = usePackageDetail(state.name, state.version, installing);
  const missing = installing && detail.data?.file_status === "missing";
  const gate = canContinue(state);
  const reason = (gate.ok ? undefined : gate.reason) ?? (missing ? MISSING : undefined) ?? blocked;
  const chosen = state.name === "" ? "" : `${state.name}@${state.version}`;

  return (
    <Stack gap={4}>
      <Visible category="P1" id="install.choose">
        <Stack gap={4}>
          <SegmentedControl
            label="What to do"
            value={state.kind}
            onChange={(kind) => set({ kind })}
            options={[
              { value: "install", label: "Install" },
              { value: "uninstall", label: "Remove" },
            ]}
          />
          {fixed && !installing ? (
            <Text block>{`Package: ${state.name} ${state.version}`}</Text>
          ) : (
            <QueryBoundary
              query={packages}
              context="load the package store"
              loading={<Skeleton shape="rows" />}
            >
              {(list) => (
                <Field label="Package">
                  <Select
                    value={chosen}
                    onChange={(event) => {
                      const at = event.target.value.lastIndexOf("@");
                      set(
                        at < 0
                          ? { name: "", version: "" }
                          : {
                              name: event.target.value.slice(0, at),
                              version: event.target.value.slice(at + 1),
                            },
                      );
                    }}
                  >
                    <option value="">Choose a package</option>
                    {chosen !== "" &&
                      !list.some((item) => `${item.name}@${item.version}` === chosen) && (
                        <option value={chosen}>{`${state.name} ${state.version}`}</option>
                      )}
                    {list.map((item) => (
                      <option
                        key={`${item.name}@${item.version}`}
                        value={`${item.name}@${item.version}`}
                      >
                        {`${item.name} ${item.version}`}
                      </option>
                    ))}
                  </Select>
                </Field>
              )}
            </QueryBoundary>
          )}
          {!installing && (
            <Text muted block size="sm">
              To remove a package that is not in the store, open the host&apos;s installed list and
              choose Remove there.
            </Text>
          )}
          {missing && (
            <InlineMessage tone="danger" title="File missing">
              {MISSING}
            </InlineMessage>
          )}
        </Stack>
      </Visible>
      <div className={styles.nav}>
        <Button variant="primary" disabledReason={reason} onClick={onNext}>
          Next
        </Button>
      </div>
    </Stack>
  );
}
