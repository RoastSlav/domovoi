import { usePreferences, useSavePreferences } from "../../../api/hooks/preferences";
import { parsePresentation } from "../../../app/visibilityCache";
import {
  Button,
  Field,
  InlineMessage,
  QueryBoundary,
  SegmentedControl,
  Select,
  Skeleton,
  Stack,
  Text,
  useActionFeedback,
} from "../../../components";
import {
  isProfileId,
  PROFILE_IDS,
  PROFILE_META,
  VISIBILITY_PAGES,
  type ProfileId,
} from "../../../design/visibility";
import styles from "../settings.module.css";
import { ProfileTable } from "./ProfileTable";

const OPTIONS = PROFILE_IDS.map((value) => ({ value, label: PROFILE_META[value].label }));

interface Save {
  message: string;
  presentation: { pages: Record<string, string>; profile: string };
}

/**
 * How much the interface shows: the default detail level, an optional level per page, what each
 * level does, and a reset. The choice is saved with the account on the hub.
 */
export function DisplaySection() {
  const preferences = usePreferences();
  const save = useActionFeedback(useSavePreferences(), {
    success: (_saved, variables: Save) => variables.message,
    error: "save the display setting",
  });
  return (
    <QueryBoundary
      query={preferences}
      context="load display settings"
      loading={<Skeleton shape="rows" />}
    >
      {(data) => {
        // Keys this build does not list are kept as received, so a save never drops them.
        const pages: Record<string, string> = { ...data.presentation.pages };
        const profile = parsePresentation(data.presentation).profile;
        const send = (presentation: Save["presentation"], message: string) =>
          save.mutate({ presentation, message });
        const isDefault = profile === "detailed" && Object.keys(pages).length === 0;
        return (
          <Stack gap={6}>
            <Stack gap={3}>
              <h3 className={styles.subhead}>Detail level</h3>
              <SegmentedControl
                label="Detail level"
                options={OPTIONS}
                value={profile}
                onChange={(next: ProfileId) =>
                  send({ profile: next, pages }, `Detail level set to ${PROFILE_META[next].label}.`)
                }
              />
              <Text muted>{PROFILE_META[profile].hint}</Text>
            </Stack>
            <Stack gap={3}>
              <h3 className={styles.subhead}>What each level shows</h3>
              <ProfileTable />
            </Stack>
            <Stack gap={3}>
              <h3 className={styles.subhead}>Per page</h3>
              {VISIBILITY_PAGES.map((page) => {
                const current = pages[page.key];
                return (
                  <Field key={page.key} label={page.label}>
                    <Select
                      value={isProfileId(current) ? current : ""}
                      onChange={(event) => {
                        const value = event.target.value;
                        const next = { ...pages };
                        if (isProfileId(value)) {
                          next[page.key] = value;
                          send(
                            { profile, pages: next },
                            `${page.label}: ${PROFILE_META[value].label}.`,
                          );
                        } else {
                          const rest = Object.fromEntries(
                            Object.entries(next).filter(([key]) => key !== page.key),
                          );
                          send(
                            { profile, pages: rest },
                            `${page.label} follows your detail level.`,
                          );
                        }
                      }}
                    >
                      <option value="">{`Same as my detail level (${PROFILE_META[profile].label})`}</option>
                      {PROFILE_IDS.map((id) => (
                        <option key={id} value={id}>
                          {PROFILE_META[id].label}
                        </option>
                      ))}
                    </Select>
                  </Field>
                );
              })}
            </Stack>
            <div>
              <Button
                variant="secondary"
                disabledReason={isDefault ? "Already using the defaults." : undefined}
                onClick={() =>
                  send({ profile: "detailed", pages: {} }, "Display reset to Detailed.")
                }
              >
                Reset to Detailed
              </Button>
            </div>
            <InlineMessage tone="info">
              These choices change what is shown, never what is allowed. Approvals, what is
              happening now, failures and anything that needs you are always shown.
            </InlineMessage>
          </Stack>
        );
      }}
    </QueryBoundary>
  );
}
