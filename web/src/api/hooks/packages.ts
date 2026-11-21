import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type PackageDetail, type PackageSummary, type PackagesStatus } from "../types";

/** The package store's status and the hub's owner key list. Answers while installation is off. */
export function usePackagesStatus() {
  return useQuery({
    queryKey: keys.packagesStatus,
    queryFn: () => call<PackagesStatus>(api.GET("/api/v1/packages/status")),
    staleTime: 30_000,
  });
}

/** Every imported package, in one request: never fetch details per row. */
export function usePackages() {
  return useQuery({
    queryKey: keys.packages,
    queryFn: () => call<PackageSummary[]>(api.GET("/api/v1/packages")),
  });
}

/** One package with its signature file and the hosts that hold it. Idle until `enabled`. */
export function usePackageDetail(name: string, version: string, enabled: boolean) {
  return useQuery({
    queryKey: keys.package(name, version),
    queryFn: () =>
      call<PackageDetail>(
        api.GET("/api/v1/packages/{name}/{version}", { params: { path: { name, version } } }),
      ),
    enabled: enabled && name !== "" && version !== "",
  });
}

/** The archive and its detached signature, as the two multipart fields the hub reads. */
export interface PackageFiles {
  archive: File;
  signature: File;
}

/** The multipart form of an import: `archive` and `signature`, each under the file's own name. */
export function packageForm(input: PackageFiles): FormData {
  const form = new FormData();
  form.append("archive", input.archive, input.archive.name);
  form.append("signature", input.signature, input.signature.name);
  return form;
}

/** What importing a package changes: the list and the store's usage. */
function useRefreshPackages() {
  const client = useQueryClient();
  return async () => {
    await Promise.all([
      client.invalidateQueries({ queryKey: keys.packages }),
      client.invalidateQueries({ queryKey: keys.packagesStatus }),
    ]);
  };
}

/** Imports an owner-signed package. Needs no password: it only fills the store. */
export function useImportPackage() {
  const onSuccess = useRefreshPackages();
  return useMutation({
    mutationFn: (input: PackageFiles) =>
      call<PackageSummary>(
        api.POST("/api/v1/packages/import", {
          // The generated type describes the form's fields; the body is sent as built.
          body: { archive: "", signature: "" },
          bodySerializer: () => packageForm(input),
        }),
      ),
    onSuccess,
  });
}

/** Deletes a package from the store. Copies already installed on hosts are untouched. */
export function useDeletePackage() {
  const onSuccess = useRefreshPackages();
  return useMutation({
    mutationFn: (input: { name: string; version: string }) =>
      call<undefined>(
        api.DELETE("/api/v1/packages/{name}/{version}", {
          params: { path: { name: input.name, version: input.version } },
        }),
      ),
    onSuccess,
  });
}
