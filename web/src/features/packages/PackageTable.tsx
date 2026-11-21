import { useState } from "react";
import { usePackages } from "../../api/hooks/packages";
import type { PackageSummary } from "../../api/types";
import {
  Button,
  DataTable,
  EmptyState,
  LinkButton,
  QueryBoundary,
  Skeleton,
  Text,
  visibilityAttrs,
  type Column,
} from "../../components";
import { PackageDetailDialog } from "./PackageDetailDialog";

/**
 * The store list: every imported package from one request. `installBlocked` is the reason an
 * install cannot start now (installation off), or undefined.
 */
export function PackageTable({ installBlocked }: { installBlocked: string | undefined }) {
  const packages = usePackages();
  const [open, setOpen] = useState<PackageSummary | undefined>(undefined);

  const columns: Column<PackageSummary>[] = [
    {
      key: "package",
      label: "Package",
      cell: (pkg) => (
        <span {...visibilityAttrs("P1", "packages.store.row")}>
          {pkg.name} <Text mono>{pkg.version}</Text>
        </span>
      ),
    },
    {
      key: "signer",
      label: "Signed by",
      cell: (pkg) => pkg.signer_label ?? <Text mono>{pkg.key_id}</Text>,
    },
    {
      key: "actions",
      label: "Actions",
      hideLabel: true,
      cell: (pkg) => (
        <span>
          {installBlocked ? (
            <Button size="sm" disabledReason={installBlocked}>
              Install…
            </Button>
          ) : (
            <LinkButton
              size="sm"
              to={`/capabilities/install?package=${encodeURIComponent(pkg.name)}&version=${encodeURIComponent(pkg.version)}`}
            >
              Install…
            </LinkButton>
          )}{" "}
          <Button size="sm" onClick={() => setOpen(pkg)}>
            Details
          </Button>
        </span>
      ),
    },
  ];

  return (
    <>
      <QueryBoundary
        query={packages}
        context="load the package store"
        loading={<Skeleton shape="rows" />}
        isEmpty={(list) => list.length === 0}
        empty={
          <EmptyState icon="package" title="No packages yet">
            <p>Import an owner-signed package above.</p>
          </EmptyState>
        }
      >
        {(list) => (
          <DataTable
            caption="Imported packages"
            columns={columns}
            rows={list}
            rowKey={(pkg) => `${pkg.name}@${pkg.version}`}
          />
        )}
      </QueryBoundary>
      {open && (
        <PackageDetailDialog
          name={open.name}
          version={open.version}
          onClose={() => setOpen(undefined)}
        />
      )}
    </>
  );
}
