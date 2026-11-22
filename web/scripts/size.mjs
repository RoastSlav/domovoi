#!/usr/bin/env node
/**
 * Fails when the built UI is over its performance budget:
 *  - the JavaScript the first page loads (gzip) is over the budget in web/DESIGN.md,
 *  - any one chunk is over 120 kB (gzip),
 *  - all CSS together is over 30 kB (gzip).
 *
 * Run it after `vite build`; `npm run build` does. `--report` prints the numbers and never fails.
 */
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dist = join(root, "dist");
const KB = 1000;
const CHUNK_LIMIT = 120 * KB;
const CSS_LIMIT = 30 * KB;

const gzip = (file) => gzipSync(readFileSync(join(dist, file))).length;
const kb = (bytes) => `${(bytes / KB).toFixed(1)} kB`;

/** The initial-JS budget written in DESIGN.md as `<!-- budget:initial-js-gzip-kb 172 -->`. */
function initialBudget() {
    const design = readFileSync(join(root, "DESIGN.md"), "utf8");
    const match = /<!--\s*budget:initial-js-gzip-kb\s+(\d+(?:\.\d+)?)\s*-->/.exec(design);
    if (!match) {
        throw new Error("DESIGN.md has no `<!-- budget:initial-js-gzip-kb N -->` line");
    }
    return Number(match[1]) * KB;
}

/** The scripts and preloads index.html names: what the browser fetches before the first paint. */
function initialScripts() {
    const html = readFileSync(join(dist, "index.html"), "utf8");
    const files = [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.js)"/g)].map((m) => m[1]);
    return [...new Set(files)];
}

const assets = readdirSync(join(dist, "assets"));
const chunks = assets
    .filter((f) => f.endsWith(".js"))
    .map((f) => ({ file: `assets/${f}`, size: gzip(`assets/${f}`) }));
const css = assets.filter((f) => f.endsWith(".css")).map((f) => gzip(`assets/${f}`));
const initial = initialScripts().reduce((sum, file) => sum + gzip(file), 0);
const cssTotal = css.reduce((sum, size) => sum + size, 0);
const largest = chunks.reduce((a, b) => (b.size > a.size ? b : a));

console.log(`Initial JS (gzip):  ${kb(initial)}`);
console.log(`Largest chunk:      ${kb(largest.size)} (${largest.file})`);
console.log(`Total CSS (gzip):   ${kb(cssTotal)}`);

const graphChunks = chunks.filter((c) => c.file.startsWith("assets/graph-"));
const graphReport = graphChunks.map((c) => `${kb(c.size)} (${c.file})`).join(", ");
console.log(`Graph chunk (gzip): ${graphChunks.length > 0 ? graphReport : "none"}`);

/** The chunks a file imports statically (`from"./x.js"` and `import"./x.js"`), not dynamic imports. */
function staticImports(file) {
    const text = readFileSync(join(dist, file), "utf8");
    const found = [...text.matchAll(/(?:from|import)\s*"\.\/([^"]+\.js)"/g)].map(
        (m) => `assets/${m[1]}`,
    );
    return found;
}

if (process.argv.includes("--report")) {
    process.exit(0);
}

const budget = initialBudget();
const problems = [];
if (initial > budget) {
    problems.push(
        `Initial JS is ${kb(initial)}, over the ${kb(budget)} budget in DESIGN.md. Load the heavy part lazily, or drop the dependency.`,
    );
}
for (const chunk of chunks.filter((c) => c.size > CHUNK_LIMIT)) {
    problems.push(
        `${chunk.file} is ${kb(chunk.size)}, over the ${kb(CHUNK_LIMIT)} chunk limit. Split it with a dynamic import.`,
    );
}
const initialFiles = initialScripts();
if (graphChunks.length === 0) {
    problems.push(
        "No graph chunk was built. The graph library must be its own lazy chunk (vite.config.ts group `graph`).",
    );
}
for (const chunk of graphChunks) {
    if (initialFiles.includes(chunk.file)) {
        problems.push(
            "The graph chunk is loaded by the first page. Import it only through StepGraph's dynamic import.",
        );
    }
    const reached = new Set();
    const queue = [...initialFiles];
    while (queue.length > 0) {
        const file = queue.pop();
        if (reached.has(file)) {
            continue;
        }
        reached.add(file);
        queue.push(...staticImports(file));
    }
    if (reached.has(chunk.file) && !initialFiles.includes(chunk.file)) {
        problems.push(`The graph chunk is statically imported by the first page (${chunk.file}).`);
    }
    for (const dep of staticImports(chunk.file)) {
        if (!dep.startsWith("assets/vendor-")) {
            problems.push(`The graph chunk depends on ${dep}; keep the library self-contained.`);
        }
    }
}
if (cssTotal > CSS_LIMIT) {
    problems.push(`CSS is ${kb(cssTotal)}, over the ${kb(CSS_LIMIT)} limit. Remove dead rules.`);
}
if (problems.length > 0) {
    console.error(`\nBundle budget exceeded:\n- ${problems.join("\n- ")}`);
    process.exit(1);
}
console.log("Within budget.");
