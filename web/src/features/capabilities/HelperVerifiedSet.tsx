import type { HelperView } from "../../api/types";
import { KeyValueList, Stack, Text, type KeyValue } from "../../components";
import { EDIT_NOTE, HUB_ADDR_NOTE, freshnessText, limitsText, listOrNone } from "./helperText";

/**
 * The effective set of a verified report, read only: the networks, protected units and
 * containers and the limits. Draws nothing without a verified report; the caller decides that.
 */
export function HelperVerifiedSet({ view }: { view: HelperView }) {
  const report = view.verified;
  if (!report) {
    return null;
  }
  const rows: KeyValue[] = [
    { label: "Networks", value: listOrNone(report.cidrs, "None configured") },
    { label: "Protected units", value: listOrNone(report.protected_units, "None") },
    { label: "Protected containers", value: listOrNone(report.protected_containers, "None") },
    { label: "Hub addresses", value: listOrNone(report.hub_addrs, "None found") },
    { label: "Containment limits", value: limitsText(view) },
    { label: "Report", value: freshnessText(view) },
  ];
  return (
    <Stack gap={2}>
      <KeyValueList items={rows} mono />
      {report.hub_addrs.length === 0 && (
        <Text block size="sm">
          {HUB_ADDR_NOTE}
        </Text>
      )}
      <Text block muted size="sm">
        {EDIT_NOTE}
      </Text>
    </Stack>
  );
}
