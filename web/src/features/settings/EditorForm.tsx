import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { useSettingsSchema, type SaveRequest } from "../../api/hooks/settings";
import { keys } from "../../api/queryClient";
import { ApiFailure, type Editable } from "../../api/types";
import {
  Button,
  ConfirmDialog,
  EmptyState,
  FormErrorSummary,
  InlineMessage,
  LinkButton,
  PasswordPrompt,
  Stack,
  SubmitBar,
  Tag,
  fieldIdForPath,
} from "../../components";
import { isSensitive } from "./form";
import { placeIssues } from "./placeIssues";
import { failureOf, type SaveFailure } from "./saveFailure";
import { SectionBody } from "./SectionBody";
import { SectionNav, type SectionState } from "./SectionNav";
import { SECTIONS, findSection } from "./sections";
import styles from "./layout.module.css";
import { useSettingsForm } from "./useSettingsForm";

/** More unsaved changes than this ask before Discard throws them away. */
const CONFIRM_DISCARD_FROM = 3;

/**
 * Every editable section as one form: the list of sections, the one the URL names, and one
 * bar to save or discard everything. The page remounts it when the saved revision changes,
 * which is what clears the edits after a save.
 */
export function EditorForm({
  editable,
  saving,
  onSave,
}: {
  editable: Editable;
  saving: boolean;
  /** Saves and applies; `count` is how many changes the request holds. */
  onSave: (request: SaveRequest, count: number) => Promise<unknown>;
}) {
  const { section: param } = useParams();
  const navigate = useNavigate();
  const client = useQueryClient();
  const schema = useSettingsSchema();
  const form = useSettingsForm(editable);
  const [problem, setProblem] = useState<SaveFailure>();
  const [attempted, setAttempted] = useState(false);
  const [asking, setAsking] = useState(false);
  const [passwordError, setPasswordError] = useState<string>();
  const [discarding, setDiscarding] = useState(false);
  const pendingFocus = useRef<string>(undefined);

  const selected = param ? findSection(param) : SECTIONS[0];
  const placed = placeIssues([...editable.issues, ...(problem?.issues ?? [])], form.visiblePaths);
  const states = new Map<string, SectionState>(
    SECTIONS.map((section) => [
      section.id,
      {
        unsaved: form.countIn(section.id),
        problems:
          placed.sections.has(section.id) ||
          (form.visiblePaths.get(section.id) ?? []).some((path) => form.problems.has(path)),
      },
    ]),
  );
  const labelOf = (path: string) => form.all.find((field) => field.path === path)?.label ?? path;
  const summary = [
    ...placed.summary,
    ...(attempted
      ? [...form.problems].map(([path, messages]) => ({
          path,
          message: `${labelOf(path)}: ${messages.join(" ")}`,
        }))
      : []),
  ];

  // After a click on a problem in another section the field exists only once that section shows.
  useEffect(() => {
    const path = pendingFocus.current;
    const field = path ? document.getElementById(fieldIdForPath(path)) : null;
    if (field) {
      field.focus();
      pendingFocus.current = undefined;
    }
  }, [selected]);

  const goToField = (event: MouseEvent) => {
    const href = (event.target as HTMLElement).closest("a")?.getAttribute("href");
    const path = summary.find(
      (item) => item.path && `#${fieldIdForPath(item.path)}` === href,
    )?.path;
    const owner = [...form.visiblePaths].find(([, paths]) => path && paths.includes(path))?.[0];
    if (path && owner && owner !== selected?.id) {
      event.preventDefault();
      void navigate(`/settings/${owner}`);
      pendingFocus.current = path;
    }
  };

  const submit = async (password?: string) => {
    setProblem(undefined);
    setPasswordError(undefined);
    try {
      await onSave(
        {
          base_revision: editable.revision,
          changes: form.changes(),
          ...(password ? { password } : {}),
        },
        form.count,
      );
      setAsking(false);
    } catch (error) {
      if (asking && error instanceof ApiFailure && error.status === 403) {
        setPasswordError("That password is not right.");
        return;
      }
      setAsking(false);
      setProblem(failureOf(error));
    }
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    setAttempted(true);
    if (form.problems.size > 0) {
      return;
    }
    if (isSensitive(form.changes())) {
      setAsking(true);
    } else {
      void submit();
    }
  };

  const context = { editable, form, issues: placed.byField, schema: schema.data };
  return (
    <>
      <form onSubmit={onSubmit} noValidate>
        <div className={styles.layout} data-view={param ? "section" : "list"}>
          <div className={styles.navColumn}>
            <SectionNav sections={SECTIONS} selected={selected?.id} states={states} />
          </div>
          <div className={styles.body}>
            <div className={styles.back}>
              <LinkButton to="/settings" variant="ghost" size="sm" icon="chevron-left">
                All settings
              </LinkButton>
            </div>
            <Stack gap={5}>
              {problem && (problem.stale || problem.issues.length === 0) && (
                <InlineMessage
                  tone="danger"
                  title={problem.title}
                  action={
                    problem.stale && (
                      <Button
                        size="sm"
                        icon="refresh"
                        onClick={() => void client.invalidateQueries({ queryKey: keys.settings })}
                      >
                        Reload
                      </Button>
                    )
                  }
                >
                  {problem.detail}
                </InlineMessage>
              )}
              <div onClickCapture={goToField}>
                <FormErrorSummary issues={summary} />
              </div>
              {selected ? (
                <SectionBody key={selected.id} section={selected} context={context} />
              ) : (
                <EmptyState
                  icon="gear"
                  title="No such section"
                  action={<Link to="/settings">Back to settings</Link>}
                >
                  <p>The address names a settings section this hub does not have.</p>
                </EmptyState>
              )}
            </Stack>
          </div>
        </div>
        <SubmitBar
          changes={form.count}
          saving={saving}
          saveLabel="Save"
          onDiscard={() =>
            form.count > CONFIRM_DISCARD_FROM ? setDiscarding(true) : form.discard()
          }
        >
          <Tag hint="Saving applies your changes by restarting the hub. It is back in a few seconds.">
            Restarts the hub
          </Tag>
        </SubmitBar>
      </form>
      {discarding && (
        <ConfirmDialog
          title={`Discard ${form.count} unsaved changes?`}
          consequence="Your edits in every section are lost. The saved settings stay as they are."
          confirmLabel="Discard changes"
          tone="danger"
          onCancel={() => setDiscarding(false)}
          onConfirm={() => {
            form.discard();
            setDiscarding(false);
          }}
        />
      )}
      {asking && (
        <PasswordPrompt
          reason="This change affects who can act or what is trusted. Enter your password to save it."
          error={passwordError}
          busy={saving}
          onConfirm={(password) => void submit(password)}
          onCancel={() => setAsking(false)}
        />
      )}
    </>
  );
}
