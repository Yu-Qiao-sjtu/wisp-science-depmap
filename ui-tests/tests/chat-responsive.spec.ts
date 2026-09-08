import { test, expect, type Page } from "@playwright/test";
import { tauriMock } from "./mock-tauri";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(tauriMock);
});

async function openConversation(page: Page) {
  await page.goto("/?mockLongSession=1");
  await page.locator(".proj-card-main").first().click();
  await expect(page.getByText("Newest page first question")).toBeVisible();
  await expect(page.locator(".composer-inner")).toBeVisible();
}

async function columnWidth(page: Page) {
  return page.locator(".composer-inner").evaluate((el) => el.getBoundingClientRect().width);
}

async function expectColumnsFit(page: Page) {
  await expect.poll(() => page.evaluate(() => {
    const pane = document.querySelector(".center")!.getBoundingClientRect();
    const thread = document.querySelector(".thread")!.getBoundingClientRect();
    const composer = document.querySelector(".composer-inner")!.getBoundingClientRect();
    const runtime = document.querySelector(".session-runtime-strip")!.getBoundingClientRect();
    const chat = document.querySelector(".chat")!;
    return {
      // Resizing across the sidebar breakpoint animates its width. Assert the
      // final pane geometry, not a transient frame with extra chat space.
      settled: [...document.querySelectorAll(".sidebar, .rightpane")]
        .every((el) => el.getAnimations().every((animation) => animation.playState !== "running")),
      aligned: Math.abs(thread.x - composer.x) <= 1
        && Math.abs(thread.width - composer.width) <= 1
        && Math.abs(runtime.x - composer.x) <= 1
        && Math.abs(runtime.width - composer.width) <= 1,
      inside: composer.left >= pane.left + 15 && composer.right <= pane.right - 15,
      noOverflow: chat.scrollWidth <= chat.clientWidth + 1
        && document.documentElement.scrollWidth <= innerWidth,
      usable: composer.width > 300 && composer.bottom <= innerHeight,
    };
  })).toEqual({ settled: true, aligned: true, inside: true, noOverflow: true, usable: true });
}

test("conversation grows on wide windows, caps on ultrawide screens, and shrinks back", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 800, height: 600 });
  await openConversation(page);
  await expectColumnsFit(page);
  const narrowWidth = await columnWidth(page);
  await page.screenshot({ path: testInfo.outputPath("narrow-conversation.png") });

  await page.setViewportSize({ width: 1440, height: 900 });
  await expectColumnsFit(page);
  // Regression: the former 920px cap wasted the additional desktop space.
  await expect.poll(() => columnWidth(page)).toBeGreaterThan(1000);
  const desktopWidth = await columnWidth(page);

  await page.setViewportSize({ width: 1920, height: 1080 });
  await expectColumnsFit(page);
  await expect.poll(() => columnWidth(page)).toBeGreaterThan(desktopWidth + 100);
  const wideWidth = await columnWidth(page);
  await page.screenshot({ path: testInfo.outputPath("wide-conversation.png") });

  await page.setViewportSize({ width: 3440, height: 1440 });
  await expectColumnsFit(page);
  await expect.poll(() => columnWidth(page)).toBeLessThanOrEqual(1281);
  expect(await columnWidth(page)).toBeCloseTo(wideWidth, 0);

  await page.setViewportSize({ width: 800, height: 600 });
  await expectColumnsFit(page);
  await expect.poll(async () => Math.abs(await columnWidth(page) - narrowWidth)).toBeLessThanOrEqual(1);
});

test("conversation follows available pane width when Inspector opens and closes", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openConversation(page);
  await expectColumnsFit(page);
  const originalWidth = await columnWidth(page);

  await page.getByRole("button", { name: "Toggle panel" }).click();
  await expect(page.locator(".rightpane")).toBeVisible();
  await expectColumnsFit(page);
  await expect.poll(() => columnWidth(page)).toBeLessThan(originalWidth - 100);

  await page.getByRole("button", { name: "Toggle panel" }).click();
  await expect(page.locator(".rightpane")).toHaveCount(0);
  await expectColumnsFit(page);
  await expect.poll(async () => Math.abs(await columnWidth(page) - originalWidth)).toBeLessThanOrEqual(1);
});
