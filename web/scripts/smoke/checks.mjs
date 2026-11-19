/** The smoke checks, in the order the brief lists them. Each one throws on the first failure. */
import { createRequire } from "node:module";
import { PLAN_HASH } from "./data.mjs";

const require = createRequire(import.meta.url);
const AXE = require.resolve("axe-core");
const WIDE = { width: 1440, height: 900 };
const PHONE = { width: 375, height: 812 };
const WAIT = { timeout: 10_000 };

function expect(condition, message) {
    if (!condition) {
        throw new Error(message);
    }
}

async function visible(locator, what) {
    try {
        await locator.first().waitFor({ state: "visible", ...WAIT });
    } catch {
        throw new Error(`Expected to see ${what}.`);
    }
}

async function open(ctx, path, viewport = WIDE) {
    await ctx.page.setViewportSize(viewport);
    await ctx.page.goto(`${ctx.base}${path}`);
}

async function goal(ctx) {
    return ctx.server.state().trace.operation.goal;
}

async function operationPage(ctx, viewport = WIDE) {
    const state = ctx.server.state();
    await open(ctx, `/operations/${state.trace.operation.id}`, viewport);
    await visible(ctx.page.getByRole("heading", { level: 1, name: await goal(ctx) }), "the goal");
}

async function noSideways(page, what) {
    const wide = await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
    );
    expect(!wide, `${what} scrolls sideways at 375 px.`);
}

async function axe(page, what) {
    await page.addScriptTag({ path: AXE });
    const violations = await page.evaluate(async () => {
        const result = await globalThis.axe.run(document, {
            resultTypes: ["violations"],
            rules: { region: { enabled: false } },
        });
        return result.violations.map((violation) => ({
            id: violation.id,
            help: violation.help,
            nodes: violation.nodes.slice(0, 3).map((node) => node.target.join(" ")),
        }));
    });
    expect(
        violations.length === 0,
        `Accessibility violations on ${what}: ${JSON.stringify(violations)}`,
    );
}

function stepLinks(page) {
    return page.locator('a[href*="step="]');
}

async function insideSlot(page) {
    return page.evaluate(() => Boolean(document.activeElement?.closest("aside, [role=dialog]")));
}

export const CHECKS = [
    {
        name: "the list shows the operation and a Legacy row",
        async run(ctx) {
            await open(ctx, "/operations");
            await visible(
                ctx.page.getByRole("heading", { level: 1, name: "Operations" }),
                "the heading",
            );
            await visible(ctx.page.getByText(await goal(ctx)), "the waiting operation");
            await visible(ctx.page.getByText("Legacy", { exact: true }), "a row tagged Legacy");
            await visible(
                ctx.page.getByRole("link", { name: "Operations", exact: true }),
                "Operations in the navigation",
            );
        },
    },
    {
        name: "the detail names the banner and the approval card",
        async run(ctx) {
            await ctx.page
                .getByRole("link", { name: await goal(ctx) })
                .first()
                .click();
            await visible(
                ctx.page.getByRole("heading", { level: 1, name: await goal(ctx) }),
                "the goal",
            );
            await visible(ctx.page.getByText("Waiting for your approval").first(), "the banner");
            const card = ctx.page.getByRole("group", { name: /Action restart_container/ });
            await visible(card, "the approval card");
            const text = await card.first().innerText();
            expect(text.includes("nas"), "The card does not name the host.");
            expect(text.includes("name = plex"), "The card does not list the arguments.");
            expect(text.includes(`Plan ${PLAN_HASH.slice(0, 8)}`), "The card has no plan prefix.");
            expect(/v2/.test(text), "The card does not say v2.");
        },
    },
    {
        name: "approving asks first and sends the full plan hash; destructive needs the name",
        async run(ctx) {
            const { page, server } = ctx;
            await page
                .getByRole("button", { name: /^Approve/ })
                .first()
                .click();
            const dialog = page.getByRole("dialog");
            await visible(dialog, "the confirmation dialog");
            expect(server.posts.length === 0, "Opening the dialog already sent a request.");
            await dialog.getByRole("button", { name: "Approve", exact: true }).click();
            for (let i = 0; i < 50 && server.posts.length === 0; i += 1) {
                await page.waitForTimeout(100);
            }
            const post = server.posts.find((entry) => entry.path.endsWith("/approve"));
            expect(post, "Confirming sent no approve request.");
            expect(
                post.body?.plan_hash === PLAN_HASH,
                "The approve body has a different plan hash.",
            );
            server.reset({ destructive: true });
            await operationPage(ctx);
            await page
                .getByRole("button", { name: /^Approve/ })
                .first()
                .click();
            const typed = page.getByRole("dialog");
            await visible(typed, "the destructive dialog");
            const confirm = typed.getByRole("button", { name: "Approve", exact: true });
            expect(
                (await confirm.getAttribute("aria-disabled")) === "true" ||
                    (await confirm.isDisabled()),
                "Confirm is enabled before the skill name is typed.",
            );
            await typed.getByRole("textbox").fill("prune_volumes");
            expect(
                (await confirm.getAttribute("aria-disabled")) !== "true" &&
                    !(await confirm.isDisabled()),
                "Confirm stays disabled with the skill name typed.",
            );
            await page.keyboard.press("Escape");
            expect(server.posts.length === 0, "A request was sent without confirming.");
        },
    },
    {
        name: "a pushed frame updates the page without a reload",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            await operationPage(ctx);
            await visible(page.getByRole("button", { name: /^Approve/ }), "Approve");
            let navigated = false;
            page.once("framenavigated", () => {
                navigated = true;
            });
            server.pauseExternally();
            await visible(page.getByText("Paused by a person").first(), "the new banner");
            expect(
                (await page.getByRole("button", { name: /^Approve/ }).count()) === 0,
                "The card still offers Approve.",
            );
            expect(!navigated, "The page reloaded.");
        },
    },
    {
        name: "a step opens the right slot, then Escape closes it and returns focus",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            await operationPage(ctx);
            const link = stepLinks(page).first();
            const href = await link.getAttribute("href");
            await link.click();
            const slot = page.getByRole("complementary", { name: "Step inspector" });
            await visible(slot, "the step inspector beside the page");
            expect(page.url().includes("step="), "The address has no step.");
            expect(await insideSlot(page), "Focus did not move into the inspector.");
            const beside = await page.evaluate(() => {
                const main = document.querySelector("main")?.getBoundingClientRect();
                const aside = document.querySelector('aside[aria-label="Step inspector"]');
                const rect = aside?.getBoundingClientRect();
                return Boolean(main && rect && rect.left >= main.right - 1);
            });
            expect(beside, "The inspector is not beside the page at 1440 px.");
            await page.keyboard.press("Escape");
            await slot.waitFor({ state: "hidden", ...WAIT });
            expect(!page.url().includes("step="), "The address kept the step.");
            const back = await page.evaluate(
                () => document.activeElement?.getAttribute("href") ?? "",
            );
            expect(back === href, `Focus did not return to the step row (it is on "${back}").`);
        },
    },
    {
        name: "no sideways scroll at 375 px, and a step opens as an inert-page sheet",
        async run(ctx) {
            const { page } = ctx;
            await open(ctx, "/operations", PHONE);
            await visible(page.getByRole("heading", { level: 1, name: "Operations" }), "the list");
            await noSideways(page, "The list");
            await operationPage(ctx, PHONE);
            await noSideways(page, "The detail page");
            await stepLinks(page).first().click();
            const sheet = page.locator('[role="dialog"][aria-modal="true"]');
            await visible(sheet, "the sheet");
            const inert = await page.evaluate(() =>
                document.querySelector("main")?.hasAttribute("inert"),
            );
            expect(inert, "The page behind the sheet is not inert.");
            await page.keyboard.press("Escape");
            await sheet.waitFor({ state: "hidden", ...WAIT });
        },
    },
    {
        name: "reduced motion makes the panel instant",
        async run(ctx) {
            const { page } = ctx;
            await page.emulateMedia({ reducedMotion: "reduce" });
            await operationPage(ctx);
            await stepLinks(page).first().click();
            await visible(
                page.getByRole("complementary", { name: "Step inspector" }),
                "the inspector",
            );
            const duration = await page.evaluate(() => {
                const panel = document.querySelector('aside[aria-label="Step inspector"] section');
                return panel ? getComputedStyle(panel).animationDuration : "missing";
            });
            await page.emulateMedia({ reducedMotion: "no-preference" });
            expect(duration === "0s", `The panel's animation lasts ${duration}, not 0s.`);
        },
    },
    {
        name: "axe finds no violations, colour contrast included",
        async run(ctx) {
            const { page } = ctx;
            await open(ctx, "/operations");
            await visible(page.getByText(await goal(ctx)), "the list");
            await axe(page, "the list");
            await operationPage(ctx);
            await axe(page, "the detail page");
            await stepLinks(page).first().click();
            await visible(
                page.getByRole("complementary", { name: "Step inspector" }),
                "the inspector",
            );
            await axe(page, "the detail page with the inspector open");
        },
    },
];
