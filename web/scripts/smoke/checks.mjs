/** The smoke checks, in the order the brief lists them. Each one throws on the first failure. */
import { createRequire } from "node:module";
import { PLAN_HASH } from "./data.mjs";
import { policyFixture } from "./server.mjs";

const require = createRequire(import.meta.url);
const AXE = require.resolve("axe-core");
const WIDE = { width: 1440, height: 900 };
const WIDE_DESK = { width: 1280, height: 900 };
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

const LOCKED_ID = "0192a000-0000-7000-8000-00000000e001";
const LIMITS_ID = "0192a000-0000-7000-8000-00000000e003";

async function bothSchemes(ctx, run) {
    try {
        for (const scheme of ["light", "dark"]) {
            await ctx.page.emulateMedia({ colorScheme: scheme });
            await run(scheme);
        }
    } finally {
        await ctx.page.emulateMedia({ colorScheme: "light" });
    }
}

async function lockedDeny(page, what) {
    await visible(page.getByText("Locked Deny", { exact: true }), `Locked Deny (${what})`);
}

async function openPublish(ctx, viewport = WIDE_DESK) {
    await open(ctx, "/policy/draft/publish", viewport);
    await visible(
        ctx.page.getByRole("heading", { level: 1, name: "Review and publish" }),
        "the review",
    );
}

async function waitForPost(server, suffix) {
    for (let i = 0; i < 50; i += 1) {
        const found = server.posts.find((entry) => entry.path.endsWith(suffix));
        if (found) return found;
        await new Promise((resolve) => setTimeout(resolve, 100));
    }
    return undefined;
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
    {
        name: "every detail level keeps the approval visible, in both schemes and widths",
        async run(ctx) {
            const { page, server } = ctx;
            try {
                for (const profile of ["detailed", "standard", "essential"]) {
                    for (const scheme of ["light", "dark"]) {
                        for (const viewport of [WIDE_DESK, PHONE]) {
                            const what = `${profile}/${scheme}/${viewport.width}px`;
                            await page.emulateMedia({ colorScheme: scheme });
                            server.reset();
                            server.setPreference(profile);
                            await operationPage(ctx, viewport);
                            await visible(
                                page.getByText("Waiting for your approval").first(),
                                `the banner (${what})`,
                            );
                            const card = page.getByRole("group", {
                                name: /Action restart_container/,
                            });
                            await visible(card, `the approval card (${what})`);
                            const text = await card.first().innerText();
                            expect(
                                text.includes(`Plan ${PLAN_HASH.slice(0, 8)}`),
                                `No plan prefix on the card (${what}).`,
                            );
                            expect(/v2/.test(text), `The card does not say v2 (${what}).`);
                            const approve = page.getByRole("button", { name: /^Approve/ }).first();
                            await visible(approve, `Approve (${what})`);
                            expect(await approve.isEnabled(), `Approve is disabled (${what}).`);

                            const folded = page.getByRole("button", {
                                name: /^\d+ technical steps?/,
                            });
                            if (profile === "detailed") {
                                expect(
                                    (await folded.count()) === 0,
                                    `Technical steps are folded in Detailed (${what}).`,
                                );
                                await visible(
                                    page.getByRole("link", { name: /Model turn|Evidence/ }),
                                    `technical step rows (${what})`,
                                );
                            } else {
                                await visible(folded, `a technical steps toggle (${what})`);
                            }
                            if (profile === "essential") {
                                await visible(
                                    page.getByRole("button", { name: /^\+\d+ details?$/ }),
                                    `a "+N details" button (${what})`,
                                );
                            }
                            const wide = await page.evaluate(
                                () =>
                                    document.documentElement.scrollWidth >
                                    document.documentElement.clientWidth,
                            );
                            expect(!wide, `The page scrolls sideways (${what}).`);
                            await axe(page, `the operation page (${what})`);
                        }
                    }
                }
            } finally {
                await page.emulateMedia({ colorScheme: "light" });
            }
        },
    },
    {
        name: "choosing Essential in the account menu saves it and changes the page in place",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await operationPage(ctx, WIDE_DESK);
            await visible(
                page.getByRole("link", { name: /Model turn|Evidence/ }),
                "technical step rows",
            );
            let navigated = false;
            page.once("framenavigated", () => {
                navigated = true;
            });
            await page.getByRole("button", { name: "Account menu" }).click();
            await page.getByRole("menuitemradio", { name: "Essential" }).click();
            for (let i = 0; i < 50 && server.puts.length === 0; i += 1) {
                await page.waitForTimeout(100);
            }
            expect(server.puts.length === 1, `Expected one PUT, saw ${server.puts.length}.`);
            const body = JSON.stringify(server.puts[0].body);
            expect(
                body === '{"presentation":{"profile":"essential","pages":{}}}',
                `The saved preference is ${body}.`,
            );
            await visible(
                page.getByRole("button", { name: /^\d+ technical steps?/ }),
                "the folded technical steps",
            );
            expect(!navigated, "The page reloaded.");
        },
    },
    {
        name: "the policy overview shows the notes, the active version, Locked Deny and decisions",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            const overview = policyFixture("overview-active");
            await open(ctx, "/policy", WIDE_DESK);
            await visible(page.getByRole("heading", { level: 1, name: "Policy" }), "the heading");
            await visible(page.getByText(overview.notes[0]), "the standing note");
            await visible(
                page.getByText(`Version ${overview.active.number}`).first(),
                "the active version",
            );
            await lockedDeny(page, "the rules table");
            await page.getByRole("tab", { name: "Decisions" }).click();
            await visible(page.getByText("skill.restart_plex").first(), "decision rows");
            for (const viewport of [PHONE, WIDE_DESK]) {
                await open(ctx, "/policy?tab=decisions", viewport);
                await visible(page.getByText("skill.restart_plex").first(), "decision rows");
                const wide = await page.evaluate(
                    () =>
                        document.documentElement.scrollWidth > document.documentElement.clientWidth,
                );
                expect(!wide, `The policy page scrolls sideways at ${viewport.width} px.`);
            }
            await bothSchemes(ctx, async (scheme) => {
                await open(ctx, "/policy", WIDE_DESK);
                await visible(page.getByText("Locked Deny", { exact: true }), "Locked Deny");
                await axe(page, `the policy overview (${scheme})`);
                await open(ctx, "/policy?tab=decisions", WIDE_DESK);
                await visible(page.getByText("skill.restart_plex").first(), "decision rows");
                await axe(page, `the decisions tab (${scheme})`);
            });
        },
    },
    {
        name: "a decision explains Locked Deny, hard limits and Not enforced, and offers no Approve",
        async run(ctx) {
            const { page } = ctx;
            await open(ctx, `/policy/decisions/${LOCKED_ID}`, WIDE_DESK);
            await visible(page.getByText("Locked Deny", { exact: true }), "Locked Deny");
            await visible(page.getByText("Not enforced", { exact: true }), "Not enforced");
            expect(
                (await page.getByRole("button", { name: /approve/i }).count()) === 0,
                "The decision page offers an Approve button.",
            );
            await axe(page, "the locked decision");
            await open(ctx, `/policy/decisions/${LIMITS_ID}`, WIDE_DESK);
            await visible(page.getByText("Hard limits", { exact: true }), "the hard-limits card");
            await visible(page.getByText("Not enforced", { exact: true }), "Not enforced");
            expect(
                (await page.getByRole("button", { name: /approve/i }).count()) === 0,
                "The limits decision offers an Approve button.",
            );
            await bothSchemes(ctx, async (scheme) => {
                await open(ctx, `/policy/decisions/${LIMITS_ID}`, WIDE_DESK);
                await visible(page.getByText("Hard limits", { exact: true }), "hard limits");
                await axe(page, `the limits decision (${scheme})`);
            });
        },
    },
    {
        name: "publishing needs every box ticked, then a password, and sends the reviewed hash",
        async run(ctx) {
            const { page, server } = ctx;
            const preview = policyFixture("preview-blocking");
            server.reset();
            await openPublish(ctx);
            const publish = page.getByRole("button", { name: /^Publish version/ });
            await visible(publish, "Publish");
            const enabled = async () =>
                (await publish.first().getAttribute("aria-disabled")) !== "true" &&
                !(await publish.first().isDisabled());
            expect(!(await enabled()), "Publish is enabled before any box is ticked.");
            const boxes = page.getByRole("checkbox", { name: /^Acknowledge:/ });
            expect(
                (await boxes.count()) === preview.blocking.length,
                `Expected ${preview.blocking.length} acknowledgement boxes, saw ${await boxes.count()}.`,
            );
            for (let i = 0; i < preview.blocking.length; i += 1) {
                expect(!(await enabled()), `Publish is enabled with only ${i} boxes ticked.`);
                await boxes.nth(i).check();
            }
            expect(await enabled(), "Publish stays disabled with every box ticked.");
            await publish.first().click();
            const dialog = page.getByRole("dialog");
            await visible(dialog, "the password dialog");
            expect(
                !server.posts.some((entry) => entry.path.endsWith("/publish")),
                "Opening the dialog already posted.",
            );
            await dialog.getByLabel("Password").fill("correct horse");
            await dialog.getByRole("button", { name: /^Publish version/ }).click();
            const post = await waitForPost(server, "/policy/draft/publish");
            expect(post, "Confirming posted nothing to /policy/draft/publish.");
            const sent = [...(post.body?.acknowledged ?? [])].sort();
            expect(
                JSON.stringify(sent) === JSON.stringify([...preview.blocking].sort()),
                `The publish body acknowledged ${JSON.stringify(sent)}.`,
            );
            expect(
                post.body?.preview_hash === preview.preview_hash,
                "The publish body has a different preview hash.",
            );
            expect(
                typeof post.body?.password === "string" && post.body.password.length > 0,
                "The publish body has no password.",
            );
            await page.waitForURL((url) => url.pathname === "/policy", WAIT);
        },
    },
    {
        name: "the simulator warns about widening, and Essential keeps the policy safety content",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await open(ctx, "/policy/simulator", WIDE_DESK);
            await page.getByRole("button", { name: "Run simulation" }).click();
            await visible(
                page.getByText(/would become Allow automatically/),
                "the widening warning",
            );
            await visible(page.getByRole("table").first(), "the results table");
            server.setPreference("essential");
            const note = policyFixture("overview-active").notes[0];
            await open(ctx, "/policy", WIDE_DESK);
            await visible(page.getByText(note), "the standing note (Essential)");
            await lockedDeny(page, "Essential");
            await openPublish(ctx);
            await visible(page.getByText(policyFixture("preview-blocking").notes[0]), "the notes");
            await visible(
                page.getByRole("checkbox", { name: /^Acknowledge:/ }),
                "the acknowledgement boxes",
            );
            await visible(page.getByRole("button", { name: /^Publish version/ }), "Publish");
            await open(ctx, `/policy/decisions/${LIMITS_ID}`, WIDE_DESK);
            await visible(page.getByText("Hard limits", { exact: true }), "the hard-limits card");
        },
    },
    {
        name: "knowledge lists facts and proposals, and a safety-related accept asks for the password first",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await open(ctx, "/knowledge", WIDE_DESK);
            await visible(
                page.getByRole("heading", { level: 1, name: "Knowledge" }),
                "the heading",
            );
            await visible(page.getByText("media server"), "the role fact");
            await page.getByRole("tab", { name: /^Proposals/ }).click();
            await visible(page.getByText("10.0.0.0/8"), "the inferred network");
            const card = page.getByRole("group", { name: /Trusted network/ });
            await card.getByRole("button", { name: "Accept" }).click();
            const dialog = page.getByRole("dialog");
            await visible(dialog.getByLabel("Password"), "the password prompt");
            expect(
                !server.posts.some((entry) => entry.path.endsWith("/accept")),
                "Opening the prompt already posted an accept.",
            );
            await dialog.getByLabel("Password").fill("correct horse");
            await dialog.getByRole("button", { name: "Accept", exact: true }).click();
            const post = await waitForPost(server, "/accept");
            expect(post, "Confirming posted no accept.");
            expect(
                post.body?.password === "correct horse" && post.body?.expected_version === 1,
                `The accept body was ${JSON.stringify({ ...post.body, password: "�" })}.`,
            );
            expect(
                !("source" in post.body) && !("actor" in post.body),
                "The accept body names a source or an actor.",
            );
            await open(ctx, "/knowledge/facts/fact-inferred-role", WIDE_DESK);
            await visible(page.getByText("Current", { exact: true }), "the status");
            await visible(page.getByText("Inferred by Domovoi").first(), "the source");
            await visible(page.getByRole("button", { name: "Make it mine" }), "Accept");
        },
    },
    {
        name: "management paths say coverage cannot be verified, block, and never send a confirm",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await open(ctx, "/knowledge/paths?host=nas", WIDE_DESK);
            await visible(page.getByText("Cannot be verified yet").first(), "the coverage pill");
            await visible(
                page.getByText(/Unattended containment is blocked on nas/),
                "the blocked message",
            );
            const confirm = page.getByRole("button", { name: /^Confirm/ }).first();
            await visible(confirm, "Confirm");
            expect(await confirm.isDisabled(), "Confirm is enabled while coverage is unavailable.");
            await confirm.click({ force: true }).catch(() => undefined);
            await new Promise((resolve) => setTimeout(resolve, 300));
            expect(
                !server.posts.some((entry) => entry.path.endsWith("/confirm")),
                "A confirm request was sent while coverage is unavailable.",
            );
            const text = await page.locator("body").innerText();
            expect(
                !/(covered|protected)/i.test(text),
                "The paths page calls a path covered or protected.",
            );
        },
    },
    {
        name: "experience shows the historical label and sources; four pages fit 375 px and pass axe in both levels and schemes",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await open(ctx, "/knowledge/experience?host=nas", WIDE_DESK);
            await visible(
                page.getByText("Historical evidence, not authorization").first(),
                "the notice",
            );
            await visible(page.getByText("restart plex after the disk filled"), "a past operation");
            await visible(
                page.getByText("Drafted by the model, not checked").first(),
                "the source of a narrative field",
            );
            const pages = [
                ["/knowledge", "Knowledge", "media server"],
                ["/knowledge/facts/fact-inferred-role", "Fact", "Inferred by Domovoi"],
                ["/knowledge/paths?host=nas", "Management paths", "Cannot be verified yet"],
                [
                    "/knowledge/experience?host=nas",
                    "Experience",
                    "restart plex after the disk filled",
                ],
            ];
            try {
                for (const profile of ["detailed", "essential"]) {
                    server.setPreference(profile);
                    for (const scheme of ["light", "dark"]) {
                        await page.emulateMedia({ colorScheme: scheme });
                        for (const [path, name, loaded] of pages) {
                            const what = `${name} (${profile}/${scheme})`;
                            await open(ctx, path, WIDE_DESK);
                            await visible(page.getByRole("heading", { level: 1 }), what);
                            await visible(page.getByText(loaded).first(), `${what} content`);
                            await axe(page, what);
                            if (profile === "detailed" && scheme === "light") {
                                await open(ctx, path, PHONE);
                                await visible(page.getByText(loaded).first(), `${what} content`);
                                await noSideways(page, name);
                            }
                        }
                    }
                }
            } finally {
                await page.emulateMedia({ colorScheme: "light" });
            }
        },
    },
    {
        name: "packages tab imports a signed package and offers to install it",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await open(ctx, "/capabilities?tab=packages", WIDE_DESK);
            await visible(page.getByRole("table", { name: "Imported packages" }), "the store list");
            await visible(page.getByText("nginx-tools").first(), "the stored package");
            await page.getByLabel("Package archive").setInputFiles({
                name: "nginx-tools-1.2.0.tar.zst",
                mimeType: "application/zstd",
                buffer: Buffer.from("archive bytes"),
            });
            await page.getByLabel("Signature file").setInputFiles({
                name: "nginx-tools-1.2.0.sig.toml",
                mimeType: "application/toml",
                buffer: Buffer.from('name = "nginx-tools"'),
            });
            await page.getByRole("button", { name: "Import package" }).click();
            await visible(page.getByText("Imported nginx-tools 1.2.0"), "the import result");
            const body = server.packages().bodies.at(-1) ?? "";
            expect(
                body.includes('name="archive"') && body.includes('name="signature"'),
                "The import was not sent as an archive and a signature field.",
            );
            await page.getByRole("link", { name: "Install it" }).click();
            await visible(
                page.getByRole("heading", { level: 1, name: /Install package/ }),
                "the wizard",
            );
            await visible(page.getByText(/nginx-tools/).first(), "the chosen package");
        },
    },
    {
        name: "install wizard shows the plan code, refuses until password and name, then follows the job to Succeeded",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await open(
                ctx,
                "/capabilities/install?package=nginx-tools&version=1.2.0&host=nas",
                WIDE_DESK,
            );
            await page.getByRole("checkbox", { name: /nas/ }).check();
            await page.getByRole("button", { name: "Next" }).click();
            await visible(page.getByText(/Plan [0-9a-f]{8}/), "the plan code");
            await page.getByRole("button", { name: "Review and confirm" }).click();
            const dialog = page.getByRole("dialog");
            const confirm = dialog.getByRole("button", { name: "Install", exact: true });
            await visible(confirm, "the confirm button");
            expect(await confirm.isDisabled(), "Confirm is enabled before the password and name.");
            await dialog.locator('input[type="password"]').fill("correct horse");
            expect(await confirm.isDisabled(), "Confirm is enabled before the name is typed.");
            await dialog.getByRole("textbox", { name: /Type nginx-tools/ }).fill("nginx-tools");
            await confirm.click();
            const post = await waitForPost(server, "/api/v1/packages/jobs");
            expect(post, "Confirming posted no install job.");
            expect(
                post.body?.plan_hash === "cd".repeat(32) &&
                    post.body?.confirm_text === "nginx-tools" &&
                    post.body?.password === "correct horse",
                `The job body was ${JSON.stringify({ ...post.body, password: "�" })}.`,
            );
            await visible(page.getByText("Running").first(), "the running status");
            await visible(page.getByText("Succeeded").first(), "the succeeded status after a poll");
        },
    },
    {
        name: "host limits are read only and the helper tab is a placeholder with no request",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            await open(ctx, "/capabilities?tab=limits&host=nas", WIDE_DESK);
            await visible(page.getByText("These limits belong to the host"), "the ownership note");
            await visible(page.getByText("container.restart").first(), "a reported limit");
            const editable = await page
                .locator("main input, main textarea, main select, main [role='switch']")
                .count();
            expect(editable === 0, "The limits tab has an editable control.");
            await open(ctx, "/capabilities?tab=helper", WIDE_DESK);
            await visible(
                page.getByText("Not available on this hub yet").first(),
                "the placeholder",
            );
            expect(
                !server.unexpected.some((key) => /helper/i.test(key)),
                "A helper request was made.",
            );
        },
    },
    {
        name: "capability center pages fit 375 px and pass axe in both schemes",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            const pages = [
                ["/capabilities?tab=packages", "Packages tab", "Imported packages"],
                [
                    "/capabilities/install?package=nginx-tools&version=1.2.0&host=nas",
                    "Install wizard",
                    "Next",
                ],
                ["/capabilities/jobs/0192b000-0000-7000-8000-00000000a001", "Job page", "nas"],
                ["/capabilities?tab=limits&host=nas", "Limits tab", "container.restart"],
                ["/capabilities?tab=helper", "Helper tab", "Not available on this hub yet"],
            ];
            try {
                for (const scheme of ["light", "dark"]) {
                    await page.emulateMedia({ colorScheme: scheme });
                    for (const [path, name, loaded] of pages) {
                        const what = `${name} (${scheme})`;
                        await open(ctx, path, WIDE_DESK);
                        await visible(page.getByText(loaded).first(), `${what} content`);
                        await axe(page, what);
                        if (scheme === "light") {
                            await open(ctx, path, PHONE);
                            await visible(page.getByText(loaded).first(), `${what} content`);
                            await noSideways(page, name);
                        }
                    }
                }
            } finally {
                await page.emulateMedia({ colorScheme: "light" });
            }
        },
    },
    {
        name: "audit page shows both histories, the whole-log check and the entry dialog",
        async run(ctx) {
            const { page, server } = ctx;
            server.reset();
            server.setPreference("detailed");
            const profiles = ["detailed", "standard", "essential"];
            for (const profile of profiles) {
                server.setPreference(profile);
                await open(ctx, "/audit", WIDE_DESK);
                await visible(page.getByRole("heading", { level: 1, name: "Audit log" }), "title");
                await visible(page.getByText("Intact").first(), "the chained status");
                await visible(page.getByText("Count matches").first(), "the legacy status");
                await visible(page.getByText("What this check can show"), "the limits note");
            }
            server.setPreference("detailed");
            await open(ctx, "/audit", WIDE_DESK);
            await visible(page.getByLabel("Head hash"), "the head hash");
            await page.getByRole("tab", { name: "Legacy history" }).click();
            await visible(page.getByRole("button", { name: "Open entry 2" }), "legacy entries");
            await page.getByRole("button", { name: "Check the whole log" }).click();
            await visible(
                page.getByText("Verified").first(),
                "the legacy status after the full check",
            );
            await page.getByRole("tab", { name: "Chained history" }).click();
            await page.getByRole("button", { name: "Open entry 7" }).click();
            await visible(page.getByRole("dialog", { name: "Audit entry #7" }), "the entry dialog");
            await visible(page.getByText("<b>marked</b>"), "the detail as text");
            await page.keyboard.press("Escape");
            try {
                for (const scheme of ["light", "dark"]) {
                    await page.emulateMedia({ colorScheme: scheme });
                    await open(ctx, "/audit", WIDE_DESK);
                    await visible(page.getByText("Intact").first(), "the status");
                    await axe(page, `Audit page (${scheme})`);
                }
                await open(ctx, "/audit", PHONE);
                await visible(page.getByText("Intact").first(), "the status");
                await noSideways(page, "Audit page");
            } finally {
                await page.emulateMedia({ colorScheme: "light" });
            }
        },
    },
    {
        name: "retention shows the nine periods read only with three marked not applied yet",
        async run(ctx) {
            const { page } = ctx;
            ctx.server.reset();
            ctx.server.setPreference("detailed");
            await open(ctx, "/settings", WIDE_DESK);
            await visible(page.getByText("Signals (days)"), "the periods");
            await visible(page.getByText("What deletion does"), "what deletion does");
            const marks = await page.getByText("Not applied yet").count();
            expect(marks === 3, `Expected 3 "Not applied yet" marks, saw ${marks}.`);
            await axe(page, "Settings retention");
            await open(ctx, "/settings", PHONE);
            await visible(page.getByText("Signals (days)"), "the periods");
            await noSideways(page, "Settings retention");
        },
    },
];
