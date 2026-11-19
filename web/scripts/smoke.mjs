#!/usr/bin/env node
/**
 * Browser smoke test: drives the built bundle in a real browser against a mock hub, to check
 * what jsdom cannot: layout, focus, the right-hand slot, reduced motion and colour contrast.
 * It is not an end-to-end test of the hub.
 *
 *   npm run build && npm run smoke
 *
 * Environment:
 *   SMOKE_CHANNEL   the installed browser to drive (default empty: Playwright's own Chromium,
 *                   after `npx playwright install chromium`); `chrome` uses an installed Chrome
 *   SMOKE_OUT       where failure screenshots go (default web/smoke-out, which git ignores)
 */
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { CHECKS } from "./smoke/checks.mjs";
import { startServer } from "./smoke/server.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dist = join(root, "dist");
const out = process.env.SMOKE_OUT ?? join(root, "smoke-out");

if (!existsSync(join(dist, "index.html"))) {
    console.error("There is no dist/ to test. Run `npm run build` first.");
    process.exit(1);
}

const server = await startServer(dist);
const browser = await chromium.launch({ channel: process.env.SMOKE_CHANNEL || undefined });
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const page = await context.newPage();
const problems = [];
page.on("console", (message) => {
    if (message.type() === "error") {
        problems.push(`console.error: ${message.text()}`);
    }
});
page.on("pageerror", (error) => problems.push(`uncaught error: ${error.message}`));

let failed = false;
const ctx = { page, context, browser, server, base: server.base };
for (const [index, check] of CHECKS.entries()) {
    try {
        await check.run(ctx);
        console.log(`ok ${index + 1}  ${check.name}`);
    } catch (error) {
        failed = true;
        mkdirSync(out, { recursive: true });
        const shot = join(out, `check-${index + 1}.png`);
        await page.screenshot({ path: shot, fullPage: true }).catch(() => undefined);
        const text = await page
            .locator("body")
            .innerText()
            .catch(() => "(no page text)");
        console.error(`FAIL ${index + 1}  ${check.name}\n${error.message}`);
        console.error(`Screenshot: ${shot}\nPage text:\n${text.slice(0, 1500)}`);
        break;
    }
}

if (!failed) {
    const extra = [...problems, ...server.unexpected.map((key) => `unexpected request: ${key}`)];
    if (extra.length === 0) {
        console.log(`ok ${CHECKS.length + 1}  no console errors and no unexpected API request`);
    } else {
        failed = true;
        console.error(`FAIL ${CHECKS.length + 1}  errors during the run:\n${extra.join("\n")}`);
    }
}

await browser.close();
server.close();
process.exit(failed ? 1 : 0);
