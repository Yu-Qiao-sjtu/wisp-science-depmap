import { test, expect, type Page } from "@playwright/test";
import { tauriMock } from "./mock-tauri";

test.use({ timezoneId: "Asia/Shanghai" });
test.beforeEach(async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-09-09T08:00:00Z"));
  await page.addInitScript(tauriMock);
});
async function open(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByTestId("open-research-calendar").click();
  await expect(page.getByTestId("home-research-calendar")).toBeVisible();
  await expect(page.getByTestId("home-calendar-details")).not.toContainText("Loading");
  return page.getByTestId("home-research-calendar");
}

test("calendar icon precedes Library; immediate Escape closes only the calendar", async ({ page }) => {
  await page.goto("/?mockLocale=zh");
  const buttons=page.locator(".projects-actions > button");
  await expect(buttons.nth(0)).toHaveAttribute("aria-label","研究日历");
  await expect(buttons.nth(0).locator("svg rect")).toHaveCount(1);
  await expect(buttons.nth(1).locator("svg")).toBeVisible();
  await expect(page.getByTestId("home-research-calendar")).toHaveCount(0);
  await buttons.nth(0).click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("home-research-calendar")).toHaveCount(0);
  await expect(page.locator(".proj-card-main").first()).toBeVisible();
  await buttons.nth(0).click();
  await page.getByRole("button",{name:"返回首页",exact:true}).click();
  await expect(page.getByTestId("home-research-calendar")).toHaveCount(0);
});

test("aggregates project marks, deduplicates sessions, filters and navigates months", async ({ page }) => {
  const calendar=await open(page);
  const details=page.getByTestId("home-calendar-details");
  await expect(calendar.getByRole("button",{name:"All projects",exact:true})).toHaveAttribute("aria-pressed","true");
  await expect(details.locator(".calendar-record-group")).toHaveCount(2);
  await expect(details).toContainText("2 projects · 11 records");
  await expect(calendar.locator('[data-date="2026-09-09"] .calendar-dot')).toHaveCount(2);
  await expect(details).toContainText("normalized_counts.csv · v2");
  await expect(details.locator(".calendar-record").filter({hasText:"Normalization method comparison"})).toHaveCount(1);
  await calendar.locator('.calendar-projects [data-project-id="other"]').click();
  await expect(details).toContainText("Other project finding");
  await expect(details).not.toContainText("normalized_counts.csv");
  await expect(calendar.locator('[data-date="2026-09-09"] .calendar-dot')).toHaveCount(1);
  await calendar.getByRole("button",{name:"2026-09-08",exact:true}).click();
  await expect(details).toContainText("Other project run failed");
  await calendar.getByRole("button",{name:"2026-09-06",exact:true}).click();
  await expect(details).toContainText("No recorded activity");
  await calendar.getByRole("button",{name:"Previous month",exact:true}).click();
  await expect(calendar.getByTestId("calendar-month")).toContainText("2026 / 08");
  await expect(details).toContainText("No recorded activity");
  await expect(calendar.locator(".calendar-grid .calendar-dot")).toHaveCount(0);
  await calendar.getByRole("button",{name:"Today",exact:true}).click();
  await expect(details).toContainText("Other project finding");
  const opens=await page.evaluate(()=>((window as any).__skillInvokeLog as any[]).filter(c=>c.cmd==="open_project"));
  expect(opens).toEqual([]);
});

test("opens the selected project and date; normal sidebar entry resets date scope", async ({ page }) => {
  const calendar=await open(page);
  await calendar.getByRole("button",{name:"2026-09-08",exact:true}).click();
  await page.getByTestId("home-calendar-details").getByRole("button",{name:"Other project · Research journey",exact:true}).click();
  const journey=page.getByTestId("research-journey");
  await expect(journey).toBeVisible();
  await expect(journey.locator(".journey-breadcrumb")).toContainText("Other project");
  await expect(journey.locator(".journey-day")).toHaveCount(1);
  await expect(journey.locator(".journey-day")).toHaveAttribute("data-day","2026-09-08");
  await page.keyboard.press("Escape");
  await expect(journey).toHaveCount(0);
  await page.locator(".sidebar").getByRole("button",{name:"Research journey",exact:true}).click();
  await expect(journey.locator(".journey-day")).toHaveCount(3);
});

test("privacy excludes hidden project records and read requests", async ({ page }) => {
  await page.addInitScript(()=>{
    localStorage.setItem("wisp-privacy-mode-active","1");
    localStorage.setItem("wisp-privacy-mode-projects",JSON.stringify(["other"]));
  });
  const calendar=await open(page);
  await expect(calendar).not.toContainText("Other project");
  const ids=await page.evaluate(()=>((window as any).__skillInvokeLog as any[]).filter(c=>c.cmd==="get_research_calendar").flatMap(c=>c.args instanceof Map?c.args.get("projectIds"):c.args.projectIds));
  expect(ids.length).toBeGreaterThan(0);expect(ids).not.toContain("other");
});

test("partial and truncated reads are explicit; refresh recovers a failed read", async ({ page }) => {
  let calendar=await open(page,"?mockCalendar=partial");
  await expect(calendar.getByRole("alert").first()).toContainText("Project temporarily unavailable");
  await expect(page.getByTestId("home-calendar-details")).toContainText("normalized_counts.csv");
  await expect(calendar.locator(".calendar-footer")).toContainText("Loaded this month");
  calendar=await open(page,"?mockCalendar=truncated");
  await expect(calendar.locator(".calendar-month")).toContainText("2,000 events");
  await expect(page.getByTestId("home-calendar-details")).not.toContainText("2,000 events");
  await page.evaluate(()=>{(window as any).__calendarError=true;});
  await calendar.getByRole("button",{name:"Refresh calendar"}).click();
  await expect(calendar.getByRole("alert").first()).toContainText("Calendar store unavailable");
  await page.evaluate(()=>{(window as any).__calendarError=false;});
  await calendar.getByRole("button",{name:"Refresh calendar"}).click();
  await expect(calendar.getByRole("alert")).toHaveCount(0);
  await expect(page.getByTestId("home-calendar-details")).toContainText("Other project finding");
});

test("closing during a read stays closed, and calendar fits light/dark narrow layouts", async ({ page }) => {
  const errors:string[]=[];page.on("pageerror",e=>errors.push(e.message));
  let calendar=await open(page,"?mockLocale=zh&mockJourney=design");
  for(const width of [1488,800,390]) {
    await page.setViewportSize({width,height:1000});
    expect(await calendar.evaluate(el=>el.scrollWidth<=el.clientWidth)).toBe(true);
    await expect(calendar.getByRole("button",{name:"2026-09-09",exact:true})).toBeVisible();
  }
  await page.evaluate(()=>document.documentElement.setAttribute("data-theme","dark"));
  expect(await calendar.evaluate(el=>el.scrollWidth<=el.clientWidth)).toBe(true);
  await page.evaluate(()=>{(window as any).__calendarDelay=200;});
  await calendar.getByRole("button",{name:"上个月",exact:true}).click();
  await page.keyboard.press("Escape");
  await expect(calendar).toHaveCount(0);
  await page.getByTestId("open-research-calendar").click();
  await expect(page.getByTestId("calendar-month")).toContainText("2026年 9月");
  await expect(page.getByTestId("home-calendar-details")).toContainText("Other project finding");
  expect(errors).toEqual([]);
});

test("daily requests follow daylight-saving day boundaries", async ({ browser }) => {
  const context=await browser.newContext({timezoneId:"America/New_York"});
  const page=await context.newPage();
  await page.clock.setFixedTime(new Date("2026-11-01T17:00:00Z"));
  await page.addInitScript(tauriMock);
  await open(page);
  const durations=await page.evaluate(()=>((window as any).__skillInvokeLog as any[]).filter(c=>c.cmd==="get_research_calendar").map(c=>{const a=c.args;return a instanceof Map?a.get("until")-a.get("from"):a.until-a.from;}));
  expect(durations).toContain(25*60*60);
  await context.close();
});


test("late month responses do not replace the newly selected month", async ({ page }) => {
  const calendar=await open(page);
  await page.evaluate(()=>{(window as any).__calendarDelay=300;});
  await calendar.getByRole("button",{name:"Previous month",exact:true}).click();
  await page.evaluate(()=>{(window as any).__calendarDelay=0;});
  await calendar.getByRole("button",{name:"Today",exact:true}).click();
  await expect(page.getByTestId("home-calendar-details")).toContainText("Other project finding");
  await page.waitForTimeout(400);
  await expect(calendar.getByTestId("calendar-month")).toContainText("2026 / 09");
  await expect(page.getByTestId("home-calendar-details")).toContainText("Other project finding");
  await expect(calendar.locator('[data-date="2026-09-09"] .calendar-dot')).toHaveCount(2);
});

test("a window palette above the calendar consumes Escape first", async ({ page }) => {
  await page.addInitScript(()=>Object.defineProperty(navigator,"platform",{get:()=>"Win32"}));
  const calendar=await open(page);
  await page.keyboard.press("Control+p");
  const palette=page.getByRole("dialog",{name:"Command Palette",exact:true});
  await expect(palette).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(palette).toHaveCount(0);
  await expect(calendar).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(calendar).toHaveCount(0);
});

test("failed project drill-down returns to a recoverable home screen", async ({ page }) => {
  await open(page);
  await page.evaluate(()=>{(window as any).__failNextProjectOpen("other");});
  await page.getByTestId("home-calendar-details").getByRole("button",{name:"Other project · Research journey",exact:true}).click();
  await expect(page.locator(".project-open-error")).toContainText("mock failed to open other");
  await expect(page.getByTestId("research-journey")).toHaveCount(0);
  await page.getByTestId("open-research-calendar").click();
  await page.getByTestId("home-calendar-details").getByRole("button",{name:"Other project · Research journey",exact:true}).click();
  await expect(page.getByTestId("research-journey")).toBeVisible();
});

test("capture home calendar visual QA", async ({ page }, testInfo) => {
  await page.setViewportSize({width:1488,height:1058});
  await page.goto("/?mockLocale=zh&mockJourney=design");
  await expect(page.locator(".proj-card-main").first()).toBeVisible();
  await page.screenshot({path:testInfo.outputPath("home.png"),animations:"disabled"});
  await page.getByTestId("open-research-calendar").click();
  const calendar=page.getByTestId("home-research-calendar");
  await expect(page.getByTestId("home-calendar-details")).toContainText("Other project finding");
  await calendar.getByRole("button",{name:"2026-09-08",exact:true}).click();
  await expect(page.getByTestId("home-calendar-details")).toContainText("Other project run failed");
  await page.screenshot({path:testInfo.outputPath("calendar-desktop.png")});
  await page.evaluate(()=>document.documentElement.setAttribute("data-theme","dark"));
  await page.screenshot({path:testInfo.outputPath("calendar-dark.png")});
  await page.setViewportSize({width:390,height:844});
  await page.screenshot({path:testInfo.outputPath("calendar-mobile.png")});
});


test("a home-owned dialog above the calendar closes before the calendar", async ({ browser }) => {
  const context=await browser.newContext({userAgent:"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/136 Safari/537.36"});
  const page=await context.newPage();
  await page.addInitScript(tauriMock);
  const calendar=await open(page);
  await page.getByRole("button",{name:"File",exact:true}).click();
  await page.getByRole("menuitem",{name:"New project"}).click();
  await expect(page.locator(".overlay .proj-settings-modal")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".overlay .proj-settings-modal")).toHaveCount(0);
  await expect(calendar).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(calendar).toHaveCount(0);
  await context.close();
});
