/**
 * A local server for the smoke test: it serves the built `dist/` and answers `/api/v1/*` as a
 * mock hub. Any `/api` request it does not know is recorded in `unexpected`, and the run fails.
 */
import { createServer } from "node:http";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import * as data from "./data.mjs";
import { makePackageState, packageRoute } from "./packages.mjs";

const policyDir = join(dirname(fileURLToPath(import.meta.url)), "../../src/test/fixtures/policy");

/** One policy fixture, read fresh so the smoke test and vitest share a single contract. */
export function policyFixture(name) {
    return JSON.parse(readFileSync(join(policyDir, `${name}.json`), "utf8"));
}

/** Decision details by execution id; every other step has none recorded. */
const DECISIONS = {
    "0192a000-0000-7000-8000-00000000e001": "decision-locked",
    "0192a000-0000-7000-8000-00000000e003": "decision-limits",
};

const TYPES = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".svg": "image/svg+xml",
    ".json": "application/json",
    ".woff2": "font/woff2",
    ".woff": "font/woff",
    ".png": "image/png",
    ".ico": "image/x-icon",
    ".map": "application/json",
};

function readBody(request) {
    return new Promise((resolve) => {
        const chunks = [];
        request.on("data", (chunk) => chunks.push(chunk));
        request.on("end", () => {
            const text = Buffer.concat(chunks).toString("utf8");
            try {
                resolve(text ? JSON.parse(text) : undefined);
            } catch {
                resolve(text);
            }
        });
    });
}

function send(response, status, body) {
    response.writeHead(status, { "content-type": "application/json" });
    response.end(JSON.stringify(body ?? {}));
}

/** Starts the server on 127.0.0.1 and returns its address and the handles the checks use. */
export async function startServer(dist) {
    let state = data.makeState();
    let packages = makePackageState(state.hostId);
    const streams = new Set();
    const posts = [];
    const unexpected = [];
    const puts = [];
    let preferences = { profile: "detailed", pages: {} };
    let preferencesAt = null;

    function preferencesState() {
        return { version: 1, updated_at: preferencesAt, presentation: preferences };
    }

    /** Bumps the journal position and tells every open stream. */
    function push(kind = "operation.changed") {
        state.seq += 1;
        const id = state.trace.operation.id;
        const frame = {
            seq: state.seq,
            aggregate_type: "operation",
            aggregate_id: id,
            kind,
            payload_v: 1,
            payload: null,
            actor: "mock",
            at: "2026-10-31T12:00:00Z",
            topics: ["operations", `operation:${id}`],
        };
        for (const stream of streams) {
            stream.write(`id: ${frame.seq}\nevent: journal\ndata: ${JSON.stringify(frame)}\n\n`);
        }
    }

    function control(path) {
        const op = state.trace.operation;
        if (path.endsWith("/approve") || path.endsWith("/deny")) {
            const approved = path.endsWith("/approve");
            state.executionState = approved ? "authorized" : "refused";
            state.approved = approved;
            op.status = approved ? "running" : "failed";
            op.waiting_for = [];
        } else if (path.endsWith("/pause")) {
            op.status = "paused";
            op.pause_reason = { type: "manual" };
        } else if (path.endsWith("/resume")) {
            op.status = "running";
            op.pause_reason = null;
        } else if (path.endsWith("/cancel")) {
            op.status = "cancelled";
        }
        push();
        return { as_of_seq: state.seq, operation: data.summary(state) };
    }

    function stream(request, response) {
        response.writeHead(200, {
            "content-type": "text/event-stream",
            "cache-control": "no-cache",
            connection: "keep-alive",
        });
        const ready = { head_seq: state.seq, resumed: true, topics: ["operations"] };
        response.write(`event: ready\ndata: ${JSON.stringify(ready)}\n\n`);
        streams.add(response);
        request.on("close", () => streams.delete(response));
    }

    /** The policy endpoints, answered from the shared fixtures; undefined when not one of them. */
    function policyRoute(method, pathname) {
        const known = {
            "GET /api/v1/policy": () => policyFixture("overview-active"),
            "GET /api/v1/policy/versions": () => ({
                items: [policyFixture("version-3").summary],
                next_cursor: null,
            }),
            "GET /api/v1/policy/draft": () => policyFixture("draft-4"),
            "GET /api/v1/policy/shadow": () => policyFixture("shadow-page"),
            "POST /api/v1/policy/preview": () => policyFixture("preview-blocking"),
            "POST /api/v1/policy/draft/publish": () => ({
                ...policyFixture("version-3").summary,
                number: 4,
            }),
            "POST /api/v1/policy/simulate": () => policyFixture("simulate-both"),
        };
        const exact = known[`${method} ${pathname}`];
        if (exact) {
            return exact;
        }
        const version = pathname.match(/^\/api\/v1\/policy\/versions\/(\d+)$/);
        if (method === "GET" && version) {
            const detail = policyFixture("version-3");
            detail.summary.number = Number(version[1]);
            return () => detail;
        }
        const decision = pathname.match(/^\/api\/v1\/policy\/shadow\/([^/]+)$/);
        if (method === "GET" && decision && DECISIONS[decision[1]]) {
            return () => policyFixture(DECISIONS[decision[1]]);
        }
        return undefined;
    }

    /** The live statuses the hub lists when no status is asked for. */
    const LIVE = ["current", "stale", "disputed"];
    const SAFETY = ["management_path", "trusted_cidr"];

    /** The knowledge and experience endpoints; undefined when the path is not one of them. */
    function knowledgeRoute(method, url, body) {
        const { pathname, searchParams } = url;
        const hostId = state.hostId;
        const ok = (payload) => ({ status: 200, payload });
        const fixture = (name) => data.knowledgeFixture(name, hostId);
        if (method === "GET" && pathname === "/api/v1/knowledge/facts") {
            const wanted = (searchParams.get("status") ?? LIVE.join(",")).split(",");
            const q = (searchParams.get("q") ?? "").toLowerCase();
            const items = state.facts.filter(
                (fact) =>
                    wanted.includes(fact.status) &&
                    (!searchParams.get("source") || fact.source === searchParams.get("source")) &&
                    (!searchParams.get("predicate") ||
                        fact.predicate === searchParams.get("predicate")) &&
                    (!searchParams.get("subject") ||
                        fact.subject === searchParams.get("subject")) &&
                    (!q ||
                        JSON.stringify(fact.value ?? "")
                            .toLowerCase()
                            .includes(q)),
            );
            return ok({ items, next_cursor: null });
        }
        const one = pathname.match(/^\/api\/v1\/knowledge\/facts\/([^/]+)$/);
        if (method === "GET" && one) {
            const found = state.facts.find((fact) => fact.id === one[1]);
            return found
                ? ok(found)
                : { status: 404, payload: { error: "not_found", message: "no such fact" } };
        }
        const act = pathname.match(/^\/api\/v1\/knowledge\/facts\/([^/]+)\/(accept|retract)$/);
        if (method === "POST" && act) {
            const found = state.facts.find((fact) => fact.id === act[1]);
            if (!found) {
                return { status: 404, payload: { error: "not_found", message: "no such fact" } };
            }
            if (act[2] === "accept") {
                if (SAFETY.includes(found.predicate) && !body?.password) {
                    return {
                        status: 403,
                        payload: { error: "forbidden", message: "this change needs your password" },
                    };
                }
                const mine = { ...found, id: `${found.id}-mine`, source: "user", actor: "user:u" };
                found.status = "superseded";
                found.stored_status = "superseded";
                state.facts.push(mine);
                return ok(mine);
            }
            found.status = "retracted";
            found.stored_status = "retracted";
            found.retracted_at = "2026-11-02T08:00:00Z";
            return ok(found);
        }
        const paths = pathname.match(/^\/api\/v1\/knowledge\/hosts\/([^/]+)\/management-paths$/);
        if (method === "GET" && paths) {
            return paths[1] === "nas"
                ? ok(fixture("paths-unavailable"))
                : { status: 404, payload: { error: "not_found", message: "no such host" } };
        }
        if (method === "GET" && pathname === "/api/v1/knowledge/relations") {
            return ok(fixture("relations-page"));
        }
        if (method === "GET" && pathname === "/api/v1/knowledge/entities") {
            return ok(fixture("entities"));
        }
        if (method === "GET" && pathname === "/api/v1/experience/search") {
            return ok(
                data.experienceFixture(
                    searchParams.get("scope") === "global" ? "search-global" : "search-host",
                ),
            );
        }
        const summary = pathname.match(/^\/api\/v1\/operations\/([^/]+)\/(summary|similar)$/);
        if (method === "GET" && summary) {
            return ok(
                summary[2] === "similar"
                    ? data.experienceFixture("similar")
                    : data.experienceFixture("summary-ready"),
            );
        }
        return undefined;
    }

    function api(request, response, url, body) {
        const { pathname } = url;
        const id = state.trace.operation.id;
        const key = `${request.method} ${pathname}`;
        if (request.method === "POST") {
            posts.push({ path: pathname, body });
        }
        if (key === "PUT /api/v1/me/preferences") {
            puts.push({ path: pathname, body });
            preferences = body?.presentation ?? preferences;
            preferencesAt = "2026-10-31T12:00:00Z";
            return send(response, 200, preferencesState());
        }
        if (key === "GET /api/v1/events") {
            return stream(request, response);
        }
        const reads = {
            "GET /api/v1/me/preferences": () => preferencesState(),
            "GET /api/v1/session": () => ({
                csrf_token: "t",
                user: { id: "u", username: "admin" },
            }),
            "GET /api/v1/hosts": () => data.hosts(state),
            "GET /api/v1/overview": () => data.overview(state),
            "GET /api/v1/system/update": () => ({ current: "0.1.0", latest: null, newer: false }),
            "GET /api/v1/settings": () => data.settings(),
            "GET /api/v1/advisories": () => [],
            "GET /api/v1/audit": () => data.auditPage(url.searchParams.get("history")),
            "GET /api/v1/audit/verification": () =>
                data.auditVerification(url.searchParams.get("scope")),
            "GET /api/v1/actions": () => ({ items: data.actions(state), next_cursor: null }),
            "GET /api/v1/operations": () => data.page(state, url.searchParams.get("status")),
            [`GET /api/v1/operations/${id}`]: () => data.detail(state),
        };
        if (reads[key]) {
            return send(response, 200, reads[key]());
        }
        const knowledge = knowledgeRoute(request.method, url, body);
        if (knowledge) {
            return send(response, knowledge.status, knowledge.payload);
        }
        const packaged = packageRoute(packages, request.method, pathname, body);
        if (packaged) {
            return send(response, packaged.status, packaged.payload);
        }
        const policy = policyRoute(request.method, pathname);
        if (policy) {
            return send(response, 200, policy());
        }
        const step = pathname.match(/^\/api\/v1\/operations\/[^/]+\/steps\/([^/]+)$/);
        if (request.method === "GET" && step) {
            const found = data.stepDetail(state, step[1]);
            return found ? send(response, 200, found) : send(response, 404, { code: "not_found" });
        }
        const commands = new RegExp(
            `^/api/v1/operations/${id}/(pause|resume|cancel|steps/[^/]+/(approve|deny|answer))$`,
        );
        if (request.method === "POST" && commands.test(pathname)) {
            return send(response, 200, control(pathname));
        }
        unexpected.push(key);
        return send(response, 404, { code: "not_found", message: "the mock hub does not know it" });
    }

    function file(response, pathname) {
        let target = normalize(join(dist, pathname));
        if (!target.startsWith(dist) || !existsSync(target) || !statSync(target).isFile()) {
            target = join(dist, "index.html");
        }
        const type = TYPES[extname(target)] ?? "application/octet-stream";
        response.writeHead(200, { "content-type": type });
        response.end(readFileSync(target));
    }

    const server = createServer(async (request, response) => {
        const url = new URL(request.url ?? "/", "http://localhost");
        if (url.pathname.startsWith("/api/")) {
            api(request, response, url, await readBody(request));
        } else {
            file(response, decodeURIComponent(url.pathname));
        }
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const { port } = server.address();
    return {
        base: `http://127.0.0.1:${port}`,
        posts,
        puts,
        unexpected,
        /** Sets the stored detail level, as if the user had chosen it earlier. */
        setPreference(profile, pages = {}) {
            preferences = { profile, pages };
            preferencesAt = "2026-10-31T12:00:00Z";
            puts.length = 0;
        },
        /** Starts over with a fresh waiting operation, optionally with a destructive skill. */
        reset(options) {
            state = data.makeState(options);
            packages = makePackageState(state.hostId);
            posts.length = 0;
        },
        /** Pauses the operation behind the page's back and tells the stream. */
        pauseExternally() {
            state.trace.operation.status = "paused";
            state.trace.operation.pause_reason = { type: "manual" };
            state.trace.operation.waiting_for = [];
            state.executionState = "refused";
            push();
        },
        state: () => state,
        packages: () => packages,
        close() {
            for (const open of streams) {
                open.end();
            }
            server.close();
        },
    };
}
