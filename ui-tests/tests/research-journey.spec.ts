import { test, expect, type Page } from "@playwright/test";
import { mkdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { tauriMock } from "./mock-tauri";

const image = readFileSync(resolve(__dirname, "../fixtures/research-comparison.png")).toString("base64");
test.use({ timezoneId: "Asia/Shanghai" });
const fixedDate = new Date("2026-09-09T08:00:00Z");
test.beforeEach(async ({ page }) => {
  await page.clock.setFixedTime(fixedDate);
  await page.addInitScript(tauriMock, { researchImageBase64: image });
});
async function open(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.locator(".proj-card-main").first().click();
  await page.locator(".sidebar").getByRole("button", { name: /Research journey|研究轨迹/, exact: true }).click();
  await expect(page.getByTestId("research-journey")).toBeVisible();
}

test("daily history groups sessions, opens exact versions and preserves Escape layers", async ({ page }) => {
  await open(page);
  const journey = page.getByTestId("research-journey");
  await expect(journey.locator(".journey-day")).toHaveCount(3);
  const today = journey.locator('[data-day="2026-09-09"]');
  await expect(today).toContainText("2 experiments · 3 outputs · 4 notes · 1 conversations");
  await expect(today.locator(".journey-session-links button")).toHaveCount(1);
  await today.getByRole("button", { name: "normalized_counts.csv", exact: true }).click();
  const source = journey.getByTestId("journey-source");
  await expect(source).toContainText("Version 2");
  await expect(source).toContainText("counts_matrix.csv");
  await source.getByRole("button", { name: "Open output", exact: true }).click();
  await expect(page.locator(".artifact-modal")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".artifact-modal")).toHaveCount(0);
  await expect(journey).toBeVisible();
  await source.getByRole("button", { name: "View run record", exact: true }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", {name: "Run record", exact: true})).toHaveCount(0);
  await expect(journey).toBeVisible();
  await journey.getByRole("button", { name: "Add entry", exact: true }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Add research entry", exact: true })).toHaveCount(0);
  await expect(journey).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(journey).toHaveCount(0);
});

test("calendar selects days and months; search and empty dates are explicit", async ({ page }) => {
  await open(page);
  const journey = page.getByTestId("research-journey");
  const calendar = journey.getByTestId("journey-calendar");
  await calendar.getByRole("button", { name: "2026-09-08", exact: true }).click();
  await expect(journey.locator(".journey-day")).toHaveCount(1);
  await expect(journey.locator(".journey-day")).toHaveAttribute("data-day", "2026-09-08");
  await journey.getByRole("button", { name: "Show full month" }).click();
  await expect(journey.locator(".journey-day")).toHaveCount(3);
  const search = journey.getByRole("searchbox", { name: "Search research records" });
  await search.fill("normalized_counts");
  await expect(journey.locator(".journey-day")).toHaveCount(1);
  await search.fill("not-present");
  await expect(journey).toContainText("No records in this view");
  await journey.getByRole("button", { name: "Previous month" }).click();
  await expect(calendar).toContainText("2026 / 08");
  await expect(journey).toContainText("No records in this view");
  await journey.getByRole("button", { name: "Back to today" }).click();
  await expect(journey.locator(".journey-day")).toHaveCount(3);
  await calendar.getByRole("button", { name: "2026-09-06", exact: true }).click();
  await expect(journey).toContainText("No records in this view");
});

test("manual backdated notes persist across reopening and show recording dates", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Add entry", exact: true }).click();
  const editor=page.getByRole("dialog", {name: "Add research entry", exact: true});
  await editor.getByLabel("Research date").fill("2026-08-31");
  await editor.getByRole("combobox", {name:"Category", exact:true}).selectOption("finding");
  await editor.getByLabel("Title", {exact:true}).fill("Baseline sensitivity observed");
  await editor.getByLabel("Details and evidence").fill("Observed in comparison run; repeat required.");
  await editor.getByRole("button", {name:"Save entry", exact:true}).click();
  await expect(editor).toHaveCount(0);
  const journey=page.getByTestId("research-journey");
  await expect(journey.getByTestId("journey-calendar")).toContainText("2026 / 08");
  await journey.locator(".journey-note-text").filter({hasText:"Baseline sensitivity observed"}).click();
  await expect(journey.getByTestId("journey-source")).toContainText("Added on 2026-09-09");
  await expect(journey.getByTestId("journey-source")).not.toContainText("12:00");
  await page.keyboard.press("Escape");
  await page.locator(".sidebar").getByRole("button", {name:"Research journey",exact:true}).click();
  await page.getByRole("button", {name:"Previous month"}).click();
  await expect(page.getByTestId("journey-feed")).toContainText("Baseline sensitivity observed");
});

test("read and save errors stay recoverable without losing entry text", async ({ page }) => {
  await open(page, "?mockJourney=error");
  await expect(page.getByRole("alert")).toContainText("Research store unavailable");
  await page.goto("/");
  await page.locator(".proj-card-main").first().click();
  await page.locator(".sidebar").getByRole("button",{name:"Research journey",exact:true}).click();
  await page.getByRole("button",{name:"Add entry",exact:true}).click();
  await page.getByLabel("Title",{exact:true}).fill("Do not lose this note");
  await page.evaluate(()=>{(window as any).__journeySaveError=true;});
  await page.getByRole("button",{name:"Save entry",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("Journal write failed");
  await expect(page.getByLabel("Title",{exact:true})).toHaveValue("Do not lose this note");
  await page.evaluate(()=>{(window as any).__journeySaveError=false;});
  await page.getByRole("button",{name:"Save entry",exact:true}).click();
  await expect(page.getByRole("dialog",{name:"Add research entry",exact:true})).toHaveCount(0);
});

test("research journey design matches the selected desktop layout and fits narrow screens", async ({ page }) => {
  const errors: string[]=[]; page.on("pageerror",e=>errors.push(e.message));
  await page.setViewportSize({width:1488,height:1058});
  await open(page,"?mockLocale=zh&mockJourney=design");
  const journey=page.getByTestId("research-journey");
  await expect(journey.locator(".journey-output-preview img").first()).toBeVisible();
  await journey.getByRole("button",{name:"normalization_comparison.png",exact:true}).click();
  await expect(journey.getByTestId("journey-source")).toContainText("归一化方法比较");
  await page.evaluate(()=>document.fonts.ready);
  await expect(journey.locator(".journey-headline").first()).toHaveCSS("font-size","18px");
  mkdirSync(resolve(__dirname,"../../docs/design-qa/research-journey"),{recursive:true});
  await page.screenshot({path:resolve(__dirname,"../../docs/design-qa/research-journey/desktop.png")});
  const comparison=await page.context().newPage();
  await comparison.setViewportSize({width:2976,height:1090});
  const before=readFileSync(resolve(__dirname,"../../docs/design-qa/research-journey/reference.png")).toString("base64");
  const after=readFileSync(resolve(__dirname,"../../docs/design-qa/research-journey/desktop.png")).toString("base64");
  await comparison.setContent(`<body style="margin:0;background:white"><div style="display:grid;grid-template-columns:1fr 1fr;font:16px sans-serif"><section><div>Selected reference</div><img style="width:100%;display:block" src="data:image/png;base64,${before}"></section><section><div>Implemented research journey</div><img style="width:100%;display:block" src="data:image/png;base64,${after}"></section></div></body>`);
  await comparison.locator("img").evaluateAll(imgs=>Promise.all(imgs.map(img=>(img as HTMLImageElement).decode())));
  await comparison.screenshot({path:resolve(__dirname,"../../docs/design-qa/research-journey/comparison.png")});
  await comparison.close();
  await page.setViewportSize({width:800,height:900});
  let bounds=await journey.boundingBox();expect(bounds!.x+bounds!.width).toBeLessThanOrEqual(800);
  await expect.poll(async()=>Math.round((await page.locator(".sidebar").boundingBox())!.width)).toBe(56);
  expect(bounds!.x).toBe(56);
  await expect(page.locator(".sidebar .side-btn.active")).toHaveCSS("color","rgba(0, 0, 0, 0)");
  expect(await journey.evaluate(el=>el.scrollWidth<=el.clientWidth)).toBe(true);
  await page.screenshot({path:resolve(__dirname,"../../docs/design-qa/research-journey/narrow.png")});
  await page.setViewportSize({width:390,height:844});
  bounds=await journey.boundingBox();expect(bounds!.x).toBe(0);expect(bounds!.width).toBeLessThanOrEqual(390);
  expect(await journey.evaluate(el=>el.scrollWidth<=el.clientWidth)).toBe(true);
  await expect(journey.locator(".journey-day").first()).toBeInViewport();
  await page.screenshot({path:resolve(__dirname,"../../docs/design-qa/research-journey/mobile.png")});
  await page.setViewportSize({width:1488,height:1058});
  await expect.poll(async()=>Math.abs((await page.locator(".sidebar").boundingBox())!.width-(await journey.boundingBox())!.x)).toBeLessThan(1);
  await page.evaluate(()=>document.documentElement.setAttribute("data-theme","dark"));
  await page.screenshot({path:resolve(__dirname,"../../docs/design-qa/research-journey/dark.png")});
  expect(errors).toEqual([]);
});

test("window dialogs stay above the research page in the Escape stack", async ({ page }) => {
  await open(page);
  const journey=page.getByTestId("research-journey");
  await page.getByRole("button",{name:"Settings",exact:true}).click();
  await page.keyboard.press("Escape");
  await expect(journey).toBeVisible();
  await page.getByRole("button",{name:"Add entry",exact:true}).click();
  await page.keyboard.press("Escape");
  await expect(journey).toBeVisible();
});

test("local calendar day bounds include the extra hour at daylight-saving end", async ({ browser }) => {
  const context=await browser.newContext({timezoneId:"America/New_York"});
  const page=await context.newPage();
  await page.clock.setFixedTime(new Date("2026-11-01T17:00:00Z"));
  await page.addInitScript(tauriMock,{researchImageBase64:image});
  await open(page);
  await page.getByTestId("journey-calendar").getByRole("button",{name:"2026-11-01",exact:true}).click();
  await expect(page.locator('.journey-day[data-day="2026-11-01"]')).toBeVisible();
  const duration=await page.evaluate(()=>{
    const args=((window as any).__skillInvokeLog as any[]).filter(c=>c.cmd==="get_research_journey").at(-1).args;
    return args instanceof Map ? args.get("until")-args.get("from") : args.until-args.from;
  });
  expect(duration).toBe(25*60*60);
  await context.close();
});

test("closing during a history request does not resurrect the page", async ({ page }) => {
  await open(page);
  const errors:string[]=[];page.on("pageerror",error=>errors.push(error.message));
  await page.evaluate(()=>{(window as any).__journeyDelay=200;});
  await page.getByRole("button",{name:"Previous month"}).click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("research-journey")).toHaveCount(0);
  await page.getByRole("button",{name:"Research journey",exact:true}).click();
  await expect(page.getByTestId("journey-calendar")).toContainText("2026 / 09");
  await expect(page.locator(".journey-day")).toHaveCount(3);
  expect(errors).toEqual([]);
});


test("large output days load previews in small groups", async ({ page }) => {
  await open(page,"?mockJourney=many");
  const outputs=page.locator('.journey-day[data-day="2026-09-09"] .journey-output');
  await expect(outputs).toHaveCount(3);
  await page.getByRole("button",{name:"Show more outputs",exact:true}).click();
  await expect(outputs).toHaveCount(8);
  await expect(page.getByRole("button",{name:"Show more outputs",exact:true})).toHaveCount(0);
});
