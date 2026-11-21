import { useMemo } from "react";
import { Link } from "react-router";
import { usePolicyVersions } from "../../api/hooks/policy";
import type { PolicyVersionSummary } from "../../api/types";
import { AllHostsNote } from "../../app/AllHostsNote";
import {
  Button,
  DataTable,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Stack,
  Tag,
  Visible,
  type Column,
} from "../../components";
import { actorText } from "./ruleText";

/**
 * Every published version, newest first, a page at a time.
 * @param activeNumber The active version, which gets an Active tag.
 */
export function VersionsTab({ activeNumber }: { activeNumber: number | undefined }) {
  const versions = usePolicyVersions();
  const columns = useMemo<Column<PolicyVersionSummary>[]>(
    () => [
      {
        key: "number",
        label: "Version",
        cell: (v) => (
          <>
            <Link to={`/policy/versions/${v.number}`}>Version {v.number}</Link>{" "}
            {v.number === activeNumber && <Tag tone="success">Active</Tag>}
          </>
        ),
      },
      { key: "at", label: "Published", cell: (v) => <RelativeTime at={v.published_at} /> },
      { key: "by", label: "By", cell: (v) => actorText(v.published_by) },
      { key: "rules", label: "Rules", numeric: true, cell: (v) => v.rule_count },
      { key: "locked", label: "Locked", numeric: true, cell: (v) => v.locked_count },
      { key: "shadow", label: "Shadow", numeric: true, cell: (v) => v.shadow_count },
    ],
    [activeNumber],
  );
  return (
    <Visible category="P1" id="policy.versions">
      <Stack gap={3}>
        <AllHostsNote>Policy applies to every host. A rule names a host itself.</AllHostsNote>
        <QueryBoundary
          query={versions}
          context="load the versions"
          loading={<Skeleton shape="rows" count={4} />}
        >
          {(data) => {
            const rows = data.pages.flatMap((page) => page.items);
            return rows.length === 0 ? (
              <EmptyState icon="shield" title="No versions yet">
                Publish a draft to create the first version.
              </EmptyState>
            ) : (
              <Stack gap={3}>
                <DataTable
                  caption="Policy versions"
                  columns={columns}
                  rows={rows}
                  rowKey={(v) => String(v.number)}
                />
                {versions.hasNextPage && (
                  <div>
                    <Button
                      onClick={() => void versions.fetchNextPage()}
                      loading={versions.isFetchingNextPage}
                    >
                      Load more
                    </Button>
                  </div>
                )}
              </Stack>
            );
          }}
        </QueryBoundary>
      </Stack>
    </Visible>
  );
}
