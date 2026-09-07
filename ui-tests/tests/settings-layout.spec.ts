import { test, expect, type Page, type Locator } from "@playwright/test";
import { tauriMock } from "./mock-tauri";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(tauriMock);
});

async function open(page: Page, label: string, locale = "en") {
  await page.goto(`/?mockLocale=${locale}`);
  await page.getByRole("button", { name: locale === "zh" ? "设置" : "Settings", exact: true }).click();
  await page.locator(".settings-nav").getByRole("button", { name: label, exact: true }).click();
}

async function fits(locator: Locator) {
  expect(await locator.evaluate(el => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
}

for (const locale of ["en", "zh"]) {
  const zh = locale === "zh";
  test(`settings cards reflow and keep controls reachable (${locale})`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1440, height: 1000 });
    await open(page, zh ? "外观" : "Appearance", locale);
    const preview = page.getByTestId("appearance-live-preview");
    const controls = page.locator(".appearance-controls");
    expect((await preview.boundingBox())!.x).toBeGreaterThan((await controls.boundingBox())!.x);
    await expect(page.getByTestId("appearance-custom-css")).toBeHidden();
    await page.screenshot({ path: testInfo.outputPath(`appearance-${locale}.png`), animations: "disabled" });
    await page.setViewportSize({ width: 820, height: 740 });
    expect((await preview.boundingBox())!.y).toBeGreaterThan((await controls.boundingBox())!.y);
    for (const field of ["appearance-ui-font", "appearance-code-font"]) {
      await page.getByTestId(field).scrollIntoViewIfNeeded();
      await fits(page.locator(".settings-appearance-pane"));
    }
    await page.getByTestId("appearance-custom-css-summary").click();
    await expect(page.getByTestId("appearance-custom-css")).toBeVisible();

    for (const [label, first, second, filename] of [
      [zh ? "记忆" : "Memory", "memory-project-card", "memory-global-card", "memory"],
      [zh ? "远程接入" : "Remote Access", "project-sync-card", "channels-overview", "remote"],
    ]) {
      await page.setViewportSize({ width: 1440, height: 1000 });
      await page.locator(".settings-nav").getByRole("button", { name: label, exact: true }).click();
      await expect(page.getByTestId(first)).toBeVisible();
      const a = (await page.getByTestId(first).boundingBox())!;
      const b = (await page.getByTestId(second).boundingBox())!;
      expect(Math.abs(a.y - b.y)).toBeLessThan(2);
      expect(b.x).toBeGreaterThan(a.x);
      await page.screenshot({ path: testInfo.outputPath(`${filename}-${locale}.png`), animations: "disabled" });
      await page.setViewportSize({ width: 820, height: 740 });
      const narrowA = (await page.getByTestId(first).boundingBox())!;
      const narrowB = (await page.getByTestId(second).boundingBox())!;
      expect(narrowB.y).toBeGreaterThanOrEqual(narrowA.y + narrowA.height);
      await fits(page.locator(".settings-content"));
    }
    const sync = page.getByTestId("project-sync-card");
    await sync.getByRole("button", { name: zh ? "保存" : "Save", exact: true }).scrollIntoViewIfNeeded();
    await fits(page.getByTestId("remote-settings-pane"));
    await page.getByTestId("feishu-channel-row").click();
    await expect(page.getByTestId("project-sync-card")).toHaveCount(0);
    await expect(page.getByTestId("feishu-channel-card")).toBeVisible();
    // One immediate Escape closes only the channel detail, retaining Settings.
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("channels-overview")).toBeVisible();
    await expect(page.getByTestId("project-sync-card")).toBeVisible();
  });
}

test("conversation groups and browser lists use content height", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await open(page, "对话", "zh");
  await expect(page.locator(".session-preferences > h3")).toHaveText(["运行限制", "上下文管理", "后续交互"]);
  expect((await page.getByTestId("max-iter").boundingBox())!.width).toBeLessThan(150);
  await page.screenshot({ path: testInfo.outputPath("session-zh.png"), animations: "disabled" });
  await page.locator(".settings-nav").getByRole("button", { name: "浏览器", exact: true }).click();
  const cards = page.locator(".browser-filter-card");
  const first = (await cards.nth(0).boundingBox())!;
  const second = (await cards.nth(1).boundingBox())!;
  expect(Math.abs(first.y - second.y)).toBeLessThan(2);
  expect(first.height).toBeLessThan(300);
  await page.setViewportSize({ width: 820, height: 740 });
  const block = (await cards.nth(0).boundingBox())!;
  const prefer = (await cards.nth(1).boundingBox())!;
  expect(prefer.y - block.y - block.height).toBeLessThan(40);
  await fits(page.getByTestId("browser-url-filters"));
});
