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
if (cssTotal > CSS_LIMIT) {
    problems.push(`CSS is ${kb(cssTotal)}, over the ${kb(CSS_LIMIT)} limit. Remove dead rules.`);
}
if (problems.length > 0) {
    console.error(`\nBundle budget exceeded:\n- ${problems.join("\n- ")}`);
    process.exit(1);
}
console.log("Within budget.");
