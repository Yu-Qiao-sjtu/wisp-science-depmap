import { expect, test } from "@playwright/test";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { resolve } from "node:path";

const repositoryRoot = resolve(__dirname, "../..");
const readRepositoryFile = (path: string) =>
  readFileSync(resolve(repositoryRoot, path), "utf8");

const skillCount = readdirSync(resolve(repositoryRoot, "skills")).filter((name) =>
  existsSync(resolve(repositoryRoot, "skills", name, "SKILL.md")),
).length;

const loadI18n = () => {
  const source = readRepositoryFile("docs/assets/i18n.js");
  const sandbox: Record<string, unknown> = {
    document: {
      addEventListener() {},
      documentElement: {
        lang: "",
        dataset: {},
        classList: { add() {} },
      },
      querySelector() {
        return null;
      },
      querySelectorAll() {
        return [];
      },
    },
    location: { search: "", href: "https://example.test/" },
    history: { replaceState() {} },
    URL,
    URLSearchParams,
    localStorage: { getItem() { return null; }, setItem() {} },
  };
  sandbox.globalThis = sandbox;
  runInNewContext(source, sandbox);
  return sandbox.WISP_PAGES_I18N as { zh: Record<string, string>; en: Record<string, string> };
};

test("GitHub Pages homepage aligns with v1.5.0 and ships a language switch", () => {
  const index = readRepositoryFile("docs/index.html");
  const i18nJs = readRepositoryFile("docs/assets/i18n.js");

  expect(index).toContain('class="lang-switch"');
  expect(index).toContain("assets/i18n.js");
  expect(index).toContain("34 个内置 SKILL");
  expect(index).toContain("v1.5.0");
  expect(index).toContain("Linux");
  expect(index).toContain("Python / R");
  expect(index).not.toContain("30 个内置");
  expect(index).not.toContain("29 bundled");
  expect(index).not.toContain("暂未签名");
  expect(index).not.toContain("仅支持从源码构建");
  expect(index).not.toContain("v0.2 仍为 beta");
  expect(index).toContain("trusted-logos/pku.svg");
  expect(index).toContain("trusted-logos/cas.svg");
  expect(index).toContain("trusted-logos/zhejiang.svg");
  expect(index).toContain("trusted-logos/washu.png");
  expect(index).toContain("trusted-logos/slu.png");
  expect(index).toContain("trusted-logos/sjtu.svg");
  expect(index).toContain("trusted-logos/meduniwien.svg");
  expect(i18nJs).toContain(`${skillCount} bundled`);
  expect(i18nJs).toContain(`${skillCount} 个内置`);
  expect(i18nJs).toContain(`${skillCount} bundled SKILL`);
});

test("Pages i18n dictionaries cover every data-i18n key and stay in sync", () => {
  const i18n = loadI18n();
  const zhKeys = Object.keys(i18n.zh).sort();
  const enKeys = Object.keys(i18n.en).sort();
  expect(zhKeys).toEqual(enKeys);

  for (const page of ["index.html", "mcp.html", "model-configuration.html", "acp-agents.html", "tutorials.html"]) {
    const html = readRepositoryFile(`docs/${page}`);
    expect(html).toContain('class="lang-switch"');
    expect(html).toContain("assets/i18n.js");
    const used = new Set(
      [...html.matchAll(/data-i18n(?:-html|-aria)?="([^"]+)"/g)].map((match) => match[1]),
    );
    const missing = [...used].filter((key) => !i18n.zh[key]).sort();
    expect(missing, page).toEqual([]);
  }
});

test("tutorials publish every WeChat article with working links, examples, and images", async ({ page }) => {
  test.setTimeout(60_000);
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== "https://tutorials.test") return route.abort();
    // Exercise the project subpath used by GitHub Pages, without network access.
    const file = resolve(repositoryRoot, "docs", url.pathname.replace(/^\/wisp-science\//, "") || "index.html");
    return existsSync(file) ? route.fulfill({ path: file }) : route.abort();
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("https://tutorials.test/wisp-science/index.html");
  await page.locator('.nav-links a[href="tutorials.html"]').click();
  await expect(page).toHaveTitle("教程 | Wisp Science");
  const sources = readdirSync(resolve(repositoryRoot, "docs/wechat")).filter((name) => name.endsWith(".md"));
  await expect(page.locator(".tutorial-article")).toHaveCount(sources.length);
  for (const name of sources) {
    const id = name.replace(/\.md$/, "");
    const title = readRepositoryFile(`docs/wechat/${name}`).split("\n")[0].replace(/^# /, "");
    await page.locator(`.tutorial-card[href="#${id}"]`).click();
    await expect(page).toHaveURL(new RegExp(`#${id}$`));
    await expect(page.locator(`#${id} h2`)).toHaveText(title);
    await expect(page.locator(`#${id} h2`)).toBeInViewport();
  }
  await page.locator('#wisp-science-trajectory p a[href="#wisp-science-skills"]').click();
  await expect(page.locator("#wisp-science-skills h2")).toBeInViewport();
  await expect(page.locator("#wisp-science-skills pre").filter({ hasText: "name: lab-paper-note" }))
    .toContainText("# 实验室论文阅读笔记");
  await expect(page.locator(".tutorial-article table").first()).toBeAttached();
  for (const img of await page.locator(".tutorial-article img").all()) {
    await img.scrollIntoViewIfNeeded();
    await expect.poll(() => img.evaluate((el: HTMLImageElement) => el.complete && el.naturalWidth > 0)).toBe(true);
  }
  await expect(page.getByRole("link", { name: "Wisp 轨迹文档", exact: true }))
    .toHaveAttribute("href", "https://github.com/xuzhougeng/wisp-science/blob/main/docs/trajectory-view.md");
  for (const width of [1440, 1280, 1120, 1101, 1100, 980, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    for (const lang of ["en", "zh"]) {
      await page.locator(`button[data-lang="${lang}"]`).click();
      await expect(page).toHaveTitle(lang === "en" ? "Tutorials | Wisp Science" : "教程 | Wisp Science");
      await expect(page.locator(".tutorial-language")).toHaveText(lang === "en"
        ? "These tutorials are currently available in Chinese." : "以下教程正文为中文。");
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      const links = page.locator(".nav-links");
      if (await links.isVisible()) {
        const brand = (await page.locator(".site-nav .brand").boundingBox())!;
        const firstLink = (await links.locator("a").first().boundingBox())!;
        const lastLink = (await links.locator("a").last().boundingBox())!;
        const controls = (await page.locator(".nav-cta").boundingBox())!;
        expect(firstLink.x).toBeGreaterThanOrEqual(brand.x + brand.width);
        expect(lastLink.x + lastLink.width).toBeLessThanOrEqual(controls.x);
      }
      await expect(page.locator('.tutorial-articles[lang="zh-CN"]')).toBeVisible();
    }
  }
  await page.locator(".tutorial-back").last().click();
  await expect(page.locator(".tutorial-card").first()).toBeInViewport();
  await page.screenshot({ path: test.info().outputPath("tutorials-mobile.png"), fullPage: false });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.evaluate(() => window.scrollTo({ top: 0, behavior: "instant" }));
  await page.screenshot({ path: test.info().outputPath("tutorials-desktop.png"), fullPage: false });
});

test("tutorial content and mobile footer entry work without JavaScript", async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 390, height: 844 } });
  const page = await context.newPage();
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== "https://tutorials.test") return route.abort();
    const file = resolve(repositoryRoot, "docs", url.pathname.slice(1) || "index.html");
    return existsSync(file) ? route.fulfill({ path: file }) : route.abort();
  });
  await page.goto("https://tutorials.test/index.html");
  await page.locator('.footer-links a[href="tutorials.html"]').click();
  await page.locator('.tutorial-card[href="#wisp-science-skills"]').click();
  await expect(page.locator("#wisp-science-skills h2")).toBeInViewport();
  await expect(page.locator("#wisp-science-skills")).toContainText("重新加载技能");
  await context.close();
});

for (const readme of ["README.md", "README_zh.md"]) {
  test(`${readme} wordmark selects a readable asset for each color scheme`, async ({ page }) => {
    await page.route("https://wordmark.test/**", (route) => route.fulfill({
      contentType: "image/svg+xml",
      body: readRepositoryFile(new URL(route.request().url()).pathname.slice(1)),
    }));
    const picture = readRepositoryFile(readme).match(/<picture>[\s\S]*?<\/picture>/)?.[0];
    expect(picture).toBeTruthy();
    await page.setContent(`<base href="https://wordmark.test/">${picture}`);
    const logo = page.getByRole("img", { name: "Wisp Science", exact: true });
    for (const mode of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: mode });
      await expect.poll(() => logo.evaluate((el: HTMLImageElement) => el.currentSrc))
        .toContain(`wordmark-${mode}.svg`);
      await expect.poll(() => logo.evaluate((el: HTMLImageElement) => el.complete && el.naturalWidth > 0)).toBe(true);
    }
  });
}

test("website hero wordmark and bilingual title fit desktop and mobile", async ({ page }) => {
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== "https://wordmark.test") return route.abort();
    const file = resolve(repositoryRoot, "docs", url.pathname.slice(1) || "index.html");
    return existsSync(file) ? route.fulfill({ path: file }) : route.abort();
  });
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("https://wordmark.test/");
  const logo = page.locator(".hero-wordmark");
  await expect(logo).toHaveAttribute("src", "assets/wordmark-light.svg");
  await expect(logo).toHaveAccessibleName("Wisp Science");
  await expect.poll(() => logo.evaluate((el: HTMLImageElement) => el.complete && el.naturalWidth > 0)).toBe(true);
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(logo).toBeVisible();
    const box = (await logo.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    expect(box.width / box.height).toBeCloseTo(520 / 344, 2);
    for (const [lang, title] of [
      ["zh", "严谨做科研， Wisp Science 在身边。"],
      ["en", "Let rigor be your guide, with Wisp Science by your side."],
    ]) {
      await page.locator(`button[data-lang="${lang}"]`).click();
      const heading = page.locator(".hero h1");
      await expect(heading).toHaveText(title, { useInnerText: true });
      await expect(heading).toBeInViewport();
      expect(await heading.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      await expect(page.locator(".hero-actions .btn-primary")).toBeInViewport();
    }
  }
});
