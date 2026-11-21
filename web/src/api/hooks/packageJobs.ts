import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type InstallJob, type InstallPlan, type JobPage } from "../types";

/** How often a running install refreshes; install jobs write journal rows no live topic carries. */
export const JOB_POLL_MS = 3000;

/** What to plan: one package on up to sixteen hosts. */
export interface PlanRequest {
  hostIds: string[];
  kind: "install" | "uninstall";
  name: string;
  version: string;
}

/** The target statuses in which a host is still working or about to. */
const WORKING = new Set(["queued", "transferring", "dispatched"]);

/** Whether `job` still changes on its own: running, or any host queued, sending or running. An unknown status is not active. */
export function isActive(job: InstallJob | undefined): boolean {
  return (
    job !== undefined &&
    (job.status === "running" || job.targets.some((target) => WORKING.has(target.status)))
  );
}

function planKey(request: PlanRequest) {
  return [
    "packages",
    "plan",
    request.kind,
    request.name,
    request.version,
    [...request.hostIds].sort(),
  ] as const;
}

/**
 * The plan the hub computes for `request`: a read-only POST used as a query. Idle while
 * `request` is undefined, and never refetched on focus so the hash does not move under the
 * person's eyes.
 */
export function usePlanJob(request: PlanRequest | undefined) {
  return useQuery({
    queryKey: request ? planKey(request) : ["packages", "plan", "idle"],
    queryFn: () => {
      if (!request) {
        throw new Error("no plan request");
      }
      return call<InstallPlan>(
        api.POST("/api/v1/packages/jobs/plan", {
          body: {
            kind: request.kind,
            name: request.name,
            version: request.version,
            host_ids: request.hostIds,
          },
        }),
      );
    },
    enabled: request !== undefined && request.hostIds.length > 0,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });
}

/** What confirming an install sends: the request, the hash that was shown, and the proof. */
export interface CreateJobInput {
  confirmText: string;
  password: string;
  planHash: string;
  request: PlanRequest;
}

/** Creates an install job. Called only by the authorization dialog; the password is never kept. */
export function useCreateInstallJob() {
  const client = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (input: CreateJobInput) =>
      call<InstallJob>(
        api.POST("/api/v1/packages/jobs", {
          body: {
            kind: input.request.kind,
            name: input.request.name,
            version: input.request.version,
            host_ids: input.request.hostIds,
            plan_hash: input.planHash,
            password: input.password,
            confirm_text: input.confirmText,
          },
        }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.installJobs }),
  });
}

/** The newest install jobs, newest first. */
export function useInstallJobs(limit: number) {
  return useQuery({
    queryKey: [...keys.installJobs, "list", limit],
    queryFn: () =>
      call<JobPage>(api.GET("/api/v1/packages/jobs", { params: { query: { limit } } })),
  });
}

/** One install job, refreshed every `JOB_POLL_MS` while a host is still working. */
export function useInstallJob(id: string) {
  return useQuery({
    queryKey: keys.installJob(id),
    queryFn: () =>
      call<InstallJob>(api.GET("/api/v1/packages/jobs/{id}", { params: { path: { id } } })),
    refetchInterval: (query) => (isActive(query.state.data) ? JOB_POLL_MS : false),
    refetchIntervalInBackground: false,
  });
}

/** What retrying one host sends: that host's plan hash and a fresh proof. */
export interface RetryInput {
  confirmText: string;
  hostId: string;
  password: string;
  planHash: string;
}

/** Runs one host's part of a job again as a new attempt. Called only by the retry dialog. */
export function useRetryTarget(jobId: string) {
  const client = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (input: RetryInput) =>
      call<InstallJob>(
        api.POST("/api/v1/packages/jobs/{id}/targets/{host_id}/retry", {
          params: { path: { id: jobId, host_id: input.hostId } },
          body: {
            plan_hash: input.planHash,
            password: input.password,
            confirm_text: input.confirmText,
          },
        }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.installJob(jobId) }),
  });
}

/** Cancels one host's part of a job that has not started. Needs no password. */
export function useCancelTarget(jobId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: { hostId: string }) =>
      call<InstallJob>(
        api.POST("/api/v1/packages/jobs/{id}/targets/{host_id}/cancel", {
          params: { path: { id: jobId, host_id: input.hostId } },
        }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.installJob(jobId) }),
  });
}
