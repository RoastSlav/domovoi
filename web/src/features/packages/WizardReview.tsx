import { useState } from "react";
import { usePlanJob, type PlanRequest } from "../../api/hooks/packageJobs";
import type { InstallPlan, InstallPlanTarget } from "../../api/types";
import { capabilityStateMeta, hostPackageStatusMeta } from "../../design/tone";
import {
  Button,
  Card,
  Cluster,
  CopyBlock,
  InlineMessage,
  KeyValueList,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  StatusPill,
  Text,
  Visible,
  visibilityAttrs,
} from "../../components";
import { AuthorizeDialog } from "./AuthorizeDialog";
import { INSTALL_NOT_ENABLE, HUMAN_ONLY, SIGNATURE_NOTE, approvalWindow } from "./packageText";
import { PlanCode } from "./PlanCode";
import styles from "./wizard.module.css";

/** Availability answers under which the hub would still try the host. */
const RUNNABLE = new Set(["available", "host_unknown", "host_absent"]);
const NONE_CAN_RUN = "None of these hosts can run this now. Go back to choose others.";

/** One host of the plan: whether it can run this, what it has now, and the hub's warnings. */
function PlanHost({ target }: { target: InstallPlanTarget }) {
  const availability = capabilityStateMeta(target.availability);
  const installed = target.installed;
  return (
    <div {...visibilityAttrs("P0", "install.review.host")}>
      <Card tone={availability.tone}>
        <Stack gap={2}>
          <Cluster gap={3}>
            <Text strong>{target.host_name}</Text>
            <StatusPill size="sm" meta={availability} />
          </Cluster>
          <Text muted size="sm" block>
            {target.availability_text}
          </Text>
          <Text block size="sm">
            {installed
              ? `Installed now: ${installed.version} (${hostPackageStatusMeta(installed.status).label})`
              : "Not installed"}
          </Text>
          {target.warnings.map((warning) => (
            <InlineMessage key={warning} tone="warning">
              {warning}
            </InlineMessage>
          ))}
        </Stack>
      </Card>
    </div>
  );
}

/** What the plan says will happen, with the signer and a short digest for an install. */
function Summary({ plan, removing }: { plan: InstallPlan; removing: boolean }) {
  const items = [
    {
      label: removing ? "Remove" : "Install",
      value: `${plan.name} ${plan.version}`,
    },
    ...(removing || !plan.key_id
      ? []
      : [
          { label: "Signed by", value: plan.signer_label ?? "a key with no label" },
          { label: "Digest", value: (plan.archive_sha256 ?? "").slice(0, 12) },
        ]),
  ];
  return <KeyValueList items={items} />;
}

/**
 * Step three: the plan the hub computed. Shows what will happen on each host, the plan code
 * and the notes, and opens the authorization dialog. It never creates a job itself.
 */
export function WizardReview({
  request,
  minutes,
  blocked,
  confirming,
  setConfirming,
  onBack,
}: {
  blocked: string | undefined;
  confirming: boolean;
  minutes: string;
  onBack: () => void;
  request: PlanRequest;
  setConfirming: (open: boolean) => void;
}) {
  const plan = usePlanJob(request);
  const [changed, setChanged] = useState(false);
  const removing = request.kind === "uninstall";
  return (
    <QueryBoundary query={plan} context="work out the plan" loading={<Skeleton shape="rows" />}>
      {(answer) => {
        const runnable = answer.targets.some((target) => RUNNABLE.has(target.availability));
        const reason = blocked ?? (runnable ? undefined : NONE_CAN_RUN);
        return (
          <Stack gap={5}>
            {changed && (
              <Visible category="P0" id="install.review.confirm">
                <InlineMessage tone="warning">The plan changed. Review it again.</InlineMessage>
              </Visible>
            )}
            <Section title="What will happen">
              <Visible category="P0" id="install.review.summary">
                <Summary plan={answer} removing={removing} />
              </Visible>
            </Section>
            <Section title="Hosts">
              <Stack gap={3}>
                {answer.targets.map((target) => (
                  <PlanHost key={target.host_id} target={target} />
                ))}
              </Stack>
            </Section>
            <Visible category="P0" id="install.review.plan">
              <PlanCode hash={answer.job_plan_hash} />
            </Visible>
            <Visible category="P0" id="install.review.notes">
              <Stack gap={2}>
                {!removing && <Text block>{INSTALL_NOT_ENABLE}</Text>}
                <Text block>{approvalWindow(minutes)}</Text>
                {!removing && <Text block>{SIGNATURE_NOTE}</Text>}
                <Text block>{HUMAN_ONLY}</Text>
              </Stack>
            </Visible>
            {!removing && answer.archive_sha256 && (
              <Visible category="P2" id="install.review.fingerprint" label="Package fingerprint">
                <KeyValueList
                  items={[
                    {
                      label: "Digest",
                      value: <CopyBlock label="Package digest" text={answer.archive_sha256} />,
                    },
                    ...(answer.key_id
                      ? [{ label: "Key id", value: <Text mono>{answer.key_id}</Text> }]
                      : []),
                  ]}
                />
              </Visible>
            )}
            <div className={styles.nav}>
              <Visible category="P0" id="install.review.confirm">
                <Cluster gap={3}>
                  <Button onClick={onBack}>Back</Button>
                  <Button
                    variant="primary"
                    disabledReason={reason}
                    onClick={() => {
                      setChanged(false);
                      setConfirming(true);
                    }}
                  >
                    Review and confirm
                  </Button>
                </Cluster>
              </Visible>
            </div>
            {confirming && (
              <AuthorizeDialog
                plan={answer}
                request={request}
                onClose={() => setConfirming(false)}
                onPlanChanged={() => {
                  setConfirming(false);
                  setChanged(true);
                  void plan.refetch();
                }}
              />
            )}
          </Stack>
        );
      }}
    </QueryBoundary>
  );
}
