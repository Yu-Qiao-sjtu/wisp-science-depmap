import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { tauriMock } from "./mock-tauri";

// Real UI with deterministic teaching data; no model API, SSH host, or browser bridge.
test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "platform", { get: () => "Linux x86_64" });
  });
  await page.addInitScript(tauriMock);
  await page.addInitScript(() => {
    const core = (window as any).__TAURI__.core;
    const invoke = core.invoke;
    core.invoke = async (cmd: string, args: any) => {
      const result = await invoke(cmd, args);
      if (cmd === "list_sessions_page") {
        result.items = [{ id: "s1", title: "示例：检查差异分析结果", ts: 1788998400, folder_id: null, has_user_turn: true, model_id: "default" }];
      }
      if (cmd === "load_session") return {
        items: [
          { role: "user", text: "请检查 results/differential_expression.csv 的列名和缺失值，暂时不要修改文件。项目联系人：李老师。", tool_name: null, ok: null },
          { role: "assistant", text: "## 数据检查记录（教学示例）\n\n表格包含 `gene`、`log2FoldChange`、`padj` 三列。\n\n| 检查项 | 示例结果 |\n| --- | --- |\n| 行数 | 1,200 |\n| log2FoldChange 缺失值 | 3 |\n| padj 缺失值 | 8 |\n\n尚未删除缺失值，也没有改写原文件。下一步可以先确认过滤规则，再绘制火山图。", tool_name: null, ok: null },
          { role: "user", text: "请先说明过滤规则，我确认后再画图。", tool_name: null, ok: null },
          { role: "assistant", text: "建议先标记缺失值，并保留原始表格。作图时单独建立有效行的副本；阈值与保留的行数一并记录。这里还没有执行绘图。", tool_name: null, ok: null },
        ], next_before_seq: null, user_offset: 0, branches: [],
      };
      return result;
    };
  });
});

async function enter(page: Page) {
  await page.goto("/?mockLocale=zh");
  await page.locator(".proj-card-main").first().click();
  await page.locator('[data-session-id="s1"]').click();
  await expect(page.locator("#composer-input")).toBeVisible();
}
async function settings(page: Page, name: string) {
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.locator(".settings-nav").getByRole("button", { name, exact: true }).click();
}
async function shot(page: Page, info: TestInfo, name: string) {
  await page.evaluate(async () => { await document.fonts.ready; });
  const path = process.env.WISP_TUTORIAL_SHOTS
    ? resolve(process.env.WISP_TUTORIAL_SHOTS, `${name}.png`)
    : info.outputPath(`${name}.png`);
  mkdirSync(dirname(path), { recursive: true });
  await page.screenshot({ path, animations: "disabled" });
}
async function emit(page: Page, name: string, payload: unknown) {
  await expect.poll(() => page.evaluate((event) => (window as any).__tauriListenerReady?.(event), name)).toBe(true);
  await page.evaluate(({ name, payload }) => (window as any).__tauriEmit(name, payload), { name, payload });
}

test("model tutorial shows API access and conversation selection", async ({ page }, info) => {
  await enter(page);
  await settings(page, "模型");
  await expect(page.getByRole("button", { name: "添加 API 接入", exact: true })).toBeVisible();
  await shot(page, info, "models/01-overview");
  await page.getByRole("button", { name: "添加 API 接入", exact: true }).click();
  await page.getByLabel(/^Base URL/).fill("https://api.deepseek.com");
  await expect(page.getByTestId("provider-model-row").first()).toBeVisible();
  await page.getByTestId("provider-model-row").first().getByLabel("模型 ID", { exact: true }).fill("deepseek-v4-flash");
  await page.getByTestId("provider-model-row").first().getByLabel("显示名称（别名）", { exact: true }).fill("实验室主模型（示例）");
  await page.getByTestId("provider-model-row").nth(1).getByLabel("模型 ID", { exact: true }).fill("deepseek-v4-pro");
  await shot(page, info, "models/02-api-access");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.locator(".model-picker-btn").click();
  await expect(page.locator(".model-menu")).toBeVisible();
  await shot(page, info, "models/03-picker");
});

test("browser tutorial shows settings and turn-end tab selection", async ({ page }, info) => {
  await enter(page);
  await settings(page, "浏览器");
  await page.getByTestId("browser-prefer-host").fill("pubmed.ncbi.nlm.nih.gov");
  await page.getByTestId("browser-prefer-add").click();
  await expect(page.getByTestId("browser-prefer-list")).toContainText("pubmed.ncbi.nlm.nih.gov");
  await shot(page, info, "browser/01-settings");
  await page.keyboard.press("Escape");
  await emit(page, "browser-tab-cleanup", {
    turn_id: "tutorial-turn", frame_id: "s1", tabs: [
      { session: "shared", tab_id: 11, url: "https://pubmed.ncbi.nlm.nih.gov/", title: "PubMed 检索页（教学示例）", initial_url: "https://pubmed.ncbi.nlm.nih.gov/" },
      { session: "shared", tab_id: 12, url: "https://example.com/", title: "Example Domain（教学示例）", initial_url: "https://example.com/" },
    ],
  });
  await expect(page.getByTestId("browser-tab-cleanup")).toBeVisible();
  await page.getByTestId("browser-tab-cleanup-check-11").uncheck();
  await shot(page, info, "browser/02-tabs");
  await page.keyboard.press("Escape");
  await emit(page, "browser-needs-human", { tabs: [{ session: "shared", tab_id: 12, url: "https://example.com/", title: "需要人工验证（教学示例）", reason: "captcha_challenge", frame_id: "s1", turn_id: "tutorial-human" }] });
  await expect(page.getByTestId("browser-needs-human")).toBeVisible();
  await shot(page, info, "browser/03-human-check");
});

test("server tutorial shows SSH settings, attachment, and terminal dock", async ({ page }, info) => {
  await enter(page);
  await settings(page, "环境");
  await page.getByRole("button", { name: "添加 SSH 主机" }).click();
  await page.locator("#add-host-alias").fill("gpu-lab");
  await page.locator("#host-hostname").fill("gpu.example.org");
  await page.locator("#host-user").fill("researcher");
  await page.locator("#host-identity").fill("~/.ssh/id_ed25519");
  await page.locator("#host-notes").fill("教学示例：长任务提交到调度器；数据保留在远程目录；运行前确认项目路径。");
  await shot(page, info, "servers/01-add-ssh");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Agent 选项" }).click();
  await page.getByRole("button", { name: /^计算环境/ }).click();
  const server = page.getByRole("menu", { name: "计算环境" }).locator('[data-context-id="ssh:gpu-server"]');
  if (!(await server.getAttribute("class"))?.includes("enabled")) await server.click();
  await expect(server).toHaveClass(/enabled/);
  await shot(page, info, "servers/02-context");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "切换面板" }).click();
  await page.locator(".rightpane").getByRole("button", { name: "环境", exact: true }).click();
  await page.locator(".context-card", { hasText: "ssh:gpu-server" }).getByRole("button", { name: "打开终端" }).click();
  await expect(page.getByTestId("terminal-dock").locator(".xterm-rows")).toContainText("terminal ready");
  await shot(page, info, "servers/03-terminal");
});

test("transfer tutorial shows project choices, session archive, and selective sharing", async ({ page }, info) => {
  await page.goto("/?mockLocale=zh");
  await page.getByRole("button", { name: "导入项目", exact: true }).click();
  await expect(page.getByTestId("project-import-options")).toBeVisible();
  await shot(page, info, "transfer/01-project-import");
  await page.keyboard.press("Escape");
  await page.locator(".proj-card:not(.proj-example)").first().getByRole("button", { name: "导出项目" }).click();
  await expect(page.getByTestId("project-export-options")).toBeVisible();
  await shot(page, info, "transfer/02-project-export");
  await page.keyboard.press("Escape");
  await page.locator(".proj-card-main").first().click();
  const session = page.locator('[data-session-id="s1"]');
  await session.click();
  await session.click({ button: "right" });
  await expect(page.locator(".ctx-menu").getByRole("button", { name: "导出会话", exact: true })).toBeVisible();
  await shot(page, info, "transfer/03-session-export");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+p");
  await page.locator("#action-palette-input").fill("导入会话归档");
  await expect(page.locator(".action-palette-row", { hasText: "导入会话归档" })).toBeVisible();
  await shot(page, info, "transfer/04-session-import");
  await page.keyboard.press("Escape");
  await page.getByTestId("share-topbar").click();
  const share = page.getByTestId("share-overlay");
  await expect(share).toBeVisible();
  await share.locator("#share-redact-input").fill("李老师");
  await share.locator(".share-row").nth(2).locator("input").uncheck();
  await share.locator(".share-row").nth(3).locator("input").uncheck();
  await expect(share.locator(".share-count")).toContainText("2/4");
  await shot(page, info, "transfer/05-share");
});
