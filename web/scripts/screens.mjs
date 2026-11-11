#!/usr/bin/env node
/**
 * Screenshots every page at four widths in both themes, and reports pages that scroll
 * sideways. Not part of CI: run it by hand after a visual change and look at the pictures.
 *
 *   cargo run -p domovoi-hub --example dev_hub      # prints the setup link
 *   npm run dev                                     # in web/
 *   SETUP_TOKEN=<token from the link> node scripts/screens.mjs
 *
 * Environment:
 *   SCREENS_URL       where the web UI is served (default http://localhost:5173)
 *   SETUP_TOKEN       the dev hub's setup token, for its first run
 *   HUB_URL           the hub itself, for that first run (default http://127.0.0.1:7420)
 *   SCREENS_USER      the account to sign in with (default "screens")
 *   SCREENS_PASSWORD  its password; a throwaway one is made when SETUP_TOKEN creates the account
 *   SCREENS_OUT       where pictures go (default web/screens-out, which git ignores)
 *   SCREENS_CHANNEL   the installed browser to drive (default "chrome"); set it empty to use
 *                     Playwright's own Chromium after `npx playwright install chromium`
 */
import { mkdirSync } from "node:fs";
import { randomBytes } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const base = process.env.SCREENS_URL ?? "http://localhost:5173";
const out = process.env.SCREENS_OUT ?? join(root, "screens-out");
const user = process.env.SCREENS_USER ?? "screens";
const password = process.env.SCREENS_PASSWORD ?? `screens-${randomBytes(9).toString("hex")}`;
const channel = process.env.SCREENS_CHANNEL ?? "chrome";

const WIDTHS = [375, 768, 1280, 1920];
const THEMES = ["light", "dark"];
const PAGES = [
    ["incidents", "/incidents"],
    ["approvals", "/approvals"],
    ["chat", "/chat"],
    ["rules", "/rules"],
    ["hosts", "/hosts"],
    ["host", "/hosts/nas"],
    ["skills", "/skills"],
    ["notes", "/notes"],
    ["digest", "/digest"],
    ["logs", "/logs"],
    ["settings", "/settings"],
    ["design", "/dev/design"],
    ["login", "/login"],
];

/** Creates the account straight on the hub: its setup check wants the hub's own address. */
async function createAccount(token) {
    const hub = process.env.HUB_URL ?? "http://127.0.0.1:7420";
    const response = await fetch(`${hub}/api/v1/auth/setup`, {
        method: "POST",
        headers: { "content-type": "application/json", origin: hub },
        body: JSON.stringify({ token, username: user, password }),
    });
    if (!response.ok) {
        throw new Error(`Setup failed: ${response.status} ${await response.text()}`);
    }
}

async function signIn(page) {
    if (process.env.SETUP_TOKEN) {
        await createAccount(process.env.SETUP_TOKEN);
    }
    await page.goto(`${base}/login`);
    await page.getByLabel("Username").fill(user);
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page.getByRole("button", { name: /log in|sign in/i }).click();
    try {
        await page.waitForURL((url) => !url.pathname.startsWith("/login"), { timeout: 10_000 });
    } catch {
        const said = await page.getByRole("alert").allInnerTexts();
        throw new Error(
            `Could not sign in as "${user}": ${said.join(" ") || "no message on the page"}`,
        );
    }
}

const browser = await chromium.launch(channel ? { channel } : {});
const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
// The hub accepts changes only from its own address; the dev server's address is another
// origin, so say the hub's when a request goes through the dev server's proxy.
const hubOrigin = process.env.HUB_URL ?? "http://127.0.0.1:7420";
await context.route(`${base}/api/**`, (route) =>
    route.continue({ headers: { ...route.request().headers(), origin: hubOrigin } }),
);
const page = await context.newPage();
mkdirSync(out, { recursive: true });
await signIn(page);

const sideways = [];
for (const theme of THEMES) {
    await page.emulateMedia({ colorScheme: theme });
    await page.addInitScript((value) => localStorage.setItem("domovoi.theme", value), theme);
    for (const width of WIDTHS) {
        await page.setViewportSize({ width, height: 900 });
        for (const [name, path] of PAGES) {
            // Not "networkidle": the live-updates stream never lets the network go quiet.
            await page.goto(`${base}${path}`, { waitUntil: "domcontentloaded" });
            await page.locator("main, form").first().waitFor();
            await page.waitForTimeout(700);
            await page.evaluate((value) => (document.documentElement.dataset.theme = value), theme);
            const overflow = await page.evaluate(
                () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
            );
            if (overflow > 0) {
                sideways.push(`${name} at ${width}px (${theme}): ${overflow}px too wide`);
            }
            await page.screenshot({
                path: join(out, `${name}-${width}-${theme}.png`),
                fullPage: true,
            });
        }
    }
}
await browser.close();

console.log(`Pictures are in ${out}`);
if (sideways.length > 0) {
    console.error(`\nPages that scroll sideways:\n- ${sideways.join("\n- ")}`);
    process.exit(1);
}
console.log("No page scrolls sideways.");
