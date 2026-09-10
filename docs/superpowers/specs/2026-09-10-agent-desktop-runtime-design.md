# Agent Desktop Runtime

> 对应 Issue：[#1193 Agent Desktop — Codex 式 PiP 监督 + 隔离 Computer Use 执行环境](https://github.com/xuzhougeng/wisp-science/issues/1193)
>
> 状态：产品形态提案；本文不代表已经实现。
>
> 一句话：PiP 是 Computer Use 的交互界面；隔离桌面（Windows ChildSession / macOS native background）是 Computer Use 的执行环境。两者不是二选一。

## 结论

Wisp Science Desktop Runtime 采用 **Codex 的 PiP 交互方式 + BetterGI 的 ChildSession 隔离思想**：

- 用户看到的只有 **Agent Desktop**。默认缩成右下角 PiP：实时画面、Agent 状态、暂停、接管、展开。
- 用户不需要理解 Session、RDP、VM、ChildSession、ScreenCaptureKit。
- Windows 默认在独立 ChildSession 里执行 Computer Use，PiP 只是该会话的实时缩略图。
- macOS 默认在主 GUI 上做后台 Computer Use（Accessibility + ScreenCaptureKit + process-targeted events），PiP 同样只是观察窗口；只有后台路径失败才 escalation 到 Isolated Runtime / VM。
- 隐藏或关闭 PiP **不得**停止底层执行 runtime。PiP 不是执行引擎。

不要做成：

```text
方案 A：PiP
VS
方案 B：ChildSession
```

而要做成：

```text
                Wisp Science
                     │
              Desktop Runtime
                     │
        ┌────────────┴─────────────┐
        │                          │
   Execution Runtime          Presentation
        │                          │
        │                          └── Agent Desktop (PiP)
        │                              │
        │                              ├─ 实时画面
        │                              ├─ Agent 状态
        │                              ├─ 暂停
        │                              ├─ 接管
        │                              └─ 展开
        │
        ├── Windows
        │      ChildSession
        │
        ├── macOS
        │      Native Background
        │      AX + ScreenCaptureKit
        │            │
        │            └─ 必要时 Isolated Runtime
        │
        └── Linux
               后续：嵌套 compositor / 独立 display
```

这样同时得到：**不抢鼠标 + 强隔离（Windows 默认，macOS 按需）+ 用户可以看见 Agent 在干什么。**

## 为什么不是二选一

PiP 和 ChildSession 解决的是两个不同层次的问题。

Codex Desktop 把 Picture-in-Picture 做成 Computer Use 的监督界面：Agent 在后台工作时，用户继续做自己的事，同时有一个小型实时预览。[OpenAI 对这套体验的描述](https://www.linkedin.com/posts/openai-devs_meet-the-new-chatgpt-experience-activity-7481042208041828353-HczL)是 “supervise the agent while it works in the background”。这对普通用户比再开一个完整“第二桌面窗口”更自然。

但 Codex 的 PiP **本身并不是“不抢鼠标”的技术原因**。公开信息里，macOS Computer Use 背后有独立的 managed service（`SkyComputerUseService`）、ScreenCaptureKit、Accessibility 权限。有人报告 Computer Use runtime 出问题时，[Hide Computer Use Picture in Picture 并不能停止底层 service](https://github.com/openai/codex/issues/38760)。

也就是说正确模型是：

```text
Computer Use Runtime
        │
        ├── screenshot
        ├── accessibility
        ├── input
        └── window control
              │
              ▼
        Target Application

               ↑
               │
        PiP 只是观察窗口
```

而不是：

```text
PiP
 ↓
执行 Computer Use
```

Codex 当前暴露的问题也说明 **PiP ≠ 隔离**：

- Stage Manager 开启时，Computer Use 可能把 inactive app 的 thumbnail 当成真实窗口，ScreenCaptureKit 拿到错误坐标并污染后续截图流（[openai/codex#38348](https://github.com/openai/codex/issues/38348)）。
- 跨应用时 scrolling / input control 丢失（[openai/codex#38508](https://github.com/openai/codex/issues/38508)）。
- 隐藏 PiP 无法停止失控的 Computer Use service，甚至可以拖垮 `launchservicesd` / WindowServer（[openai/codex#38760](https://github.com/openai/codex/issues/38760)）。

独立 Desktop Session 在这些点上更强，但把第二桌面直接暴露给用户，理解成本过高。Wisp 的产品选择是：**底层做强隔离，上层做得像 PiP 一样轻。**

| 层 | Codex 风格 | BetterGI 风格 | Wisp 选择 |
| --- | --- | --- | --- |
| 用户界面 | **PiP** | RDP / 桌面分身窗口 | **Agent Desktop PiP** |
| 截图 | ScreenCaptureKit 等 | ChildSession 屏幕 | 平台 adapter |
| 输入 | macOS Computer Use runtime | ChildSession 内 SendInput | 平台 adapter |
| 执行环境 | 主 macOS GUI 为主 | 独立 ChildSession | Windows 默认 ChildSession；macOS 默认 native background，失败再隔离 |
| 用户监督 | **很好** | 一般 | **很好** |
| 强隔离 | 一般 | **很好** | Windows **很好**；macOS 默认一般、按需升级 |
| 资源开销 | **低** | 较低 | 跟随平台默认路径 |
| 用户理解成本 | **低** | 较高 | **低**（UI 不出现 ChildSession / RDP / VM） |

## 产品语义

### 用户看到的名词

只使用 **Agent Desktop**。

禁止在 UI、设置、工具结果、错误文案里默认出现：ChildSession、RDP、VM、桌面分身、ScreenCaptureKit、Accessibility。诊断/日志可以保留内部名称，面向用户的失败应翻译成“Agent Desktop 无法后台操作该窗口”这类可行动描述。

默认缩成右下角：

```text
┌─────────────────┐
│ ● Agent Desktop │
│                 │
│   [实时画面]    │
│                 │
│ Running  02:31  │
│                 │
│ 接管  暂停  展开│
└─────────────────┘
```

点击 **展开** 才看到完整 Agent Desktop。展开后的窗口仍然是 Wisp 的监督/接管表面，不是系统远程桌面客户端。

### 控件语义

| 动作 | 对 Presentation | 对 Execution Runtime |
| --- | --- | --- |
| 隐藏 PiP | 停止显示画面 | **继续运行** |
| 关闭展开窗口 | 回到 PiP | **继续运行** |
| 暂停 | 画面仍可刷新，状态变为 Paused | 停止向目标应用发送输入；保持会话 |
| 接管 | 用户输入进入 Agent Desktop | Agent 输入暂停；用户操作隔离桌面（Windows）或被授权的目标窗口（macOS） |
| 结束 / 停止 Agent Desktop | 关闭 PiP 与展开窗口 | **显式**拆除 runtime、回收进程和会话 |
| Escape | 只关闭当前最上层表面（展开 → PiP；菜单 → 展开）。不结束 runtime | 无 |

这是从 Codex #38760 直接学到的约束：Presentation 生命周期 ≠ Runtime 生命周期。

### 状态机

```text
Idle
  → Starting
  → Running
  → Paused          （用户暂停或审批门）
  → Takeover        （用户正在操作 Agent Desktop）
  → Escalating      （macOS 后台路径失败，正在切换隔离 runtime）
  → Failed
  → Stopping
  → Stopped
```

`Idle` / `Stopped` 时不显示 PiP。`Starting` 即可显示 PiP 占位（无画面也要有状态）。`Failed` 保留最后一帧和错误，直到用户关闭或重试。

同一时刻每个 OS 用户会话最多一个 **活动** Agent Desktop。Windows ChildSession 本身也限制同一时间只能有一个 connected child session；产品层不要承诺并行多个隔离桌面。

## 与现有能力的边界

Agent Desktop 是新的 GUI Computer Use 平面，不是现有平面的改名。

| 现有能力 | 继续负责 | 不交给 Agent Desktop 的 v0 |
| --- | --- | --- |
| Browser Runtime（shared / workspace Chrome） | 用户日常浏览器、已登录 Cookie、DOM/CSS 选择器、扩展桥 | 不把默认 `browser-use` 改道到像素级桌面 |
| Python / R Runtime | 持久解释器 | 不在 Agent Desktop 里“画” REPL |
| ExecutionContext `local` / `wsl` / `ssh` | shell、文件、Run | GUI 应用不是 SSH 命令 |
| #1061 父子会话 | 对话/任务派发 | 名字里的 ChildSession 与 Windows 终端服务 Child Session 无关 |

科研场景里，网页任务仍走 Browser Runtime。ImageJ、RStudio、Excel、PowerPoint、PyMOL、IGV、Finder、以及扩展桥够不到的窗口，走 Agent Desktop。

后续可以把 workspace Chrome **放进** Agent Desktop 里跑，那是集成工作，不是 v0 替换。

## 分层

### Presentation：Agent Desktop PiP

只负责：

- 实时或节流后的画面
- 状态、耗时、当前目标应用名
- 暂停 / 接管 / 展开 / 隐藏
- 审批与“需要你看一眼”的提示
- 把用户在接管模式下的输入转发给 Execution Runtime

不负责：

- 截图采集实现
- 鼠标键盘注入
- 窗口枚举
- 启动/回收隔离会话

### Execution Runtime：平台 adapter

统一能力，测试用 fake runner 实现：

```text
create / attach / stop
capture_frame
list_windows / get_window
accessibility_snapshot?   （平台可空）
input_click / input_type / input_scroll / input_key
focus_window
pause_input / resume_input
enable_takeover / disable_takeover
probe_capabilities
```

Windows adapter 默认把这些操作打进 ChildSession。macOS adapter 默认打进 native background。Linux adapter v0 可以只返回 `unsupported`，但 Presentation 和 DTO 必须能在 Linux 上编译和显示“此平台尚未提供 Agent Desktop 执行环境”。

### 控制平面

放在 desktop shell（`src-tauri`）+ 共享 DTO（`crates/wisp-dto`），不要塞进 `wisp-runtime`（那是 Python/R）或 Browser Runtime。

建议新模块，例如 `src-tauri/src/agent_desktop/`，而不是把 GUI 会话塞进 `execution_contexts` 的 `local` 记录。Agent Desktop 更接近 Runtime（长寿命交互环境），但产品名词保持独立，避免和 Python/R Runtime 卡片混在一起。

持久化最小集合：

- `id`
- `project_id`、`frame_id`（可空：桌面会话可以跨对话保留，但 v0 绑定当前项目）
- `status`
- `platform` / `backend`（`windows_child_session` | `macos_native` | `macos_isolated` | `linux_unsupported` | `fake`）
- `started_at`、`updated_at`、`stopped_at`
- `last_error`
- `target_app`、`elapsed` 的投影可以是内存态，重启后不恢复画面流

v0 不要求跨应用重启后重新 attach 到同一个 ChildSession。进程退出则会话结束，与当前 Python/R Runtime 一致。

## 平台策略

### Windows：ChildSession + PiP，两个都要

后台：

```text
Windows ChildSession
┌────────────────────────┐
│ Chrome / ImageJ / ...  │
│                        │
│ Agent 鼠标 → 点击     │
│ Agent 键盘 → 输入     │
└────────────────────────┘
```

前台用户看到的是 ChildSession 的实时缩略图，不是用户主桌面。

实现约束（来自 Windows 终端服务 Child Sessions）：

- 使用前必须 `WTSEnableChildSessions`
- 同一时间只能有一个 active connected child session
- 从当前用户会话 loopback 创建，不把私钥/密码写入 SQLite
- ChildSession 随父会话结束而结束
- Agent 的 `SendInput` / 截图必须针对 child session 的桌面，而不是用户正在用的桌面
- 自动化测试禁止要求真实 ChildSession；用 fake runner + 纯解析/状态机测试

这比 Codex 当前 macOS 方案更强：

```text
Codex:     Same Desktop → Computer Use → PiP
Wisp Win:  Isolated Desktop → Computer Use → PiP Monitor
```

### macOS：默认不要开 VM

优先学 Codex 的后台路径：

```text
macOS Desktop Runtime
ScreenCaptureKit + Accessibility + process-targeted events
      │
      ▼
 Background Computer Use
      │
      ▼
     PiP
```

绝大多数科研 GUI 应尽量后台完成：Safari、Chrome、Finder、ImageJ、RStudio、Excel、PowerPoint、PyMOL、IGV。

只有探测到下列情况才 escalation：

```text
必须前台
必须真实 HID
Canvas 操作失败
Accessibility tree 不完整
窗口无法后台交互
```

```text
Native Background
       ↓ fail
Isolated Desktop / VM
```

v0 只需要把 escalation **建模出来**（状态 `Escalating`、错误码、用户可见原因）。真正的 macOS VM / 独立桌面是后续 PR，不要在第一个 macOS adapter 里打包虚拟机。

macOS 实现必须显式处理 Codex 已经踩过的坑：

- 拒绝 Stage Manager thumbnail / 屏外窗口作为 capture target
- 捕获失败不能污染后续整条截图流
- 窗口引用过期后要重新 bind，而不是对已消失的 window id 继续 Send
- managed service 启动必须 single-flight，禁止 PiP 或功能开关触发 spawn storm
- 关闭 PiP ≠ 停止 service；停止必须走显式 teardown，并在测试里覆盖这条不变量

### Linux

v0 交付 Presentation 契约和 `unsupported` backend。不要在没有选定 compositor 方案之前假装有隔离桌面。后续候选（不写入 v0 验收）：嵌套 Wayland compositor、独立 X display、或用户已有的 headless GUI。

## 科研目标应用（验收导向，不是白名单）

能后台完成就后台完成，不要求用户把这些应用拖到前台：

- Safari / Chrome / Finder
- ImageJ
- RStudio
- Excel / PowerPoint
- PyMOL
- IGV

v0 不保证每个应用都能无前台操作。每个平台 adapter 必须能返回结构化失败（`needs_foreground`、`ax_incomplete`、`hid_required`、`canvas_failed`），供 Agent 和 PiP 显示，而不是卡死或乱点用户桌面。

## UI 约束

- 图标走 `compose_icon()`，接管 / 暂停 / 展开 / 隐藏必须使用不同 kind。
- PiP 是持续监督表面，不是普通 modal。展开视图、PiP 菜单、接管确认必须进入窗口级 Escape 栈：先关最上层，父级仍在。测试必须在打开后立刻按 Escape，不先把焦点移进内部。
- 隐藏 PiP 与停止 Agent Desktop 是两个控件，禁止复用同一图标或同一命令。
- 画面是远程帧，不是把 ChildSession HWND 嵌进 WebView 后让用户误以为那是执行引擎。
- 跟随现有窗口隔离：PiP 只出现在拥有该 Agent Desktop 的项目窗口（与 #1074 / #1079 一致）。

## 工具面（后于控制平面）

先有 Desktop Runtime 和 PiP，再暴露 Agent 工具。建议独立工具名，避免和 `browser_setup` / `web_*` 混淆，例如 `agent_desktop`：

- `status` / `start` / `stop`
- `screenshot`
- `act`（click / type / scroll / key，针对 Agent Desktop 内窗口）
- `pause` / `resume` / `request_takeover`

v0 工具必须声明当前 backend 和是否隔离。禁止在用户主桌面注入输入，除非 macOS native backend 已明确目标窗口且用户不在 Takeover。

审批：Computer Use 属于高副作用。沿用现有审批模型，不要用 PiP 上的“看起来像在跑”代替授权。

## 分阶段实施

每一阶段都必须可单独评审、可测试、不要求真实 ChildSession / VM / Accessibility 权限。

### Phase A — 控制平面与 DTO

- [ ] `AgentDesktopSession` 状态机、backend 枚举、错误码
- [ ] `wisp-dto` 形状；`src-tauri` 契约测试
- [ ] SQLite 最小表（幂等 migration）
- [ ] fake runner：create/pause/takeover/stop
- [ ] 不变量测试：hide presentation 不变 runtime status

### Phase B — Agent Desktop PiP 壳

- [ ] 右下角 PiP：状态、耗时、占位画面、接管/暂停/展开/隐藏
- [ ] 展开窗口
- [ ] Escape 栈与图标
- [ ] Playwright：打开后立刻 Escape；隐藏 ≠ 停止
- [ ] 无活动会话时不显示 PiP

### Phase C — Windows ChildSession adapter

- [ ] 真实 adapter 与 fake 分开；CI 默认 fake
- [ ] 能力探测：ChildSession 是否可用（SKU / 策略）
- [ ] 在 child desktop 内启动目标应用、截图、SendInput
- [ ] PiP 订阅 child 帧
- [ ] 文档：用户只看到 Agent Desktop

### Phase D — macOS native background adapter

- [ ] ScreenCaptureKit 帧 + AX 快照的接口边界
- [ ] process-targeted events，禁止默认全局 HID
- [ ] 拒绝 Stage Manager thumbnail / 屏外窗口
- [ ] capture 失败隔离（一条流失败不得毒化后续）
- [ ] runtime 启动 single-flight
- [ ] `Escalating` 状态与用户可见原因；v0 可以失败关闭而不是真的起 VM

### Phase E — Agent 工具与科研 GUI 冒烟

- [ ] `agent_desktop` 工具
- [ ] 与 Browser Runtime 的路由说明（网页默认仍走 extension）
- [ ] 手工冒烟：Excel / ImageJ / 浏览器窗口至少各一条（不进 CI）
- [ ] 失败码可被模型理解并显示在 PiP

### Phase F — macOS isolated runtime / Linux backend

- [ ] 仅当 Phase D 的失败模式被真实科研应用打满后再做
- [ ] Linux 选定一种嵌套 GUI 方案后再实现，不提前抽象“跨三平台 VM”

## 验收标准

- [ ] 用户文案只有 Agent Desktop，没有 ChildSession / RDP / VM
- [ ] Windows：Agent 在隔离桌面点击时，用户主桌面鼠标不被抢走
- [ ] 任何平台：PiP 显示的是 Execution Runtime 的画面，而不是“PiP 自己在点”
- [ ] 隐藏 PiP 后 runtime 仍 Running；停止只能通过显式停止
- [ ] 暂停后不再发送输入；接管后 Agent 输入让路给用户
- [ ] 展开后 Escape 回到 PiP，会话仍在
- [ ] macOS 默认不启动 VM；后台失败给出可行动原因
- [ ] 自动化测试不依赖真实 SSH、GPU、SLURM、WSL、ChildSession、Accessibility 权限或网络
- [ ] 默认网页任务仍走 Browser Runtime，不被 Agent Desktop 截胡

## 非目标

- 不在一个 PR 里同时做 PiP、ChildSession、macOS AX、VM
- 不把 Browser Runtime 或 Python/R Runtime 迁进 Agent Desktop
- 不把 Windows ChildSession 的实现细节暴露成用户设置项
- 不把“第二桌面窗口 / 远程桌面客户端”当作 v0 主界面
- 不默认在 macOS 上开 VM
- 不把 PiP 当作可以单独执行 Computer Use 的运行时
- 不要求 Linux v0 具备真实隔离桌面
- 不把密钥、RDP 密码、SSH 私钥写入 SQLite
- 不通过拉长 `shell` 超时来模拟 GUI 任务

## 关键决策

1. **分层而不是二选一。** PiP 解决监督与理解成本；ChildSession / native background 解决输入隔离与坐标真实性。
2. **用户名词是 Agent Desktop。** 内部 backend 名可以平台化，UI 不能。
3. **Presentation 杀不死 Runtime。** 来自 Codex #38760。
4. **Windows 默认隔离，macOS 默认后台。** 科研 GUI 大多数可以后台完成；macOS 没有低成本 ChildSession 等价物，VM 是 escalation 不是默认。
5. **同一时刻一个活动 Agent Desktop。** 对齐 Windows 限制，也避免多个 PiP 抢监督注意力。
6. **不替换 Browser Runtime。** DOM 级网页自动化继续走 extension；像素/AX 级原生应用才走桌面。
7. **测试全部可假。** 真实 ChildSession / ScreenCaptureKit 只存在于手工冒烟和平台 gated 测试。

## PR Plan

### PR 1 — Agent Desktop 控制平面

- 标题：Add Agent Desktop session model and fake runner
- 影响：`crates/wisp-dto`、`crates/wisp-store`、`src-tauri/src/agent_desktop/`、契约测试
- 依赖：无
- 内容：状态机、DTO、幂等表、fake runner、hide≠stop 测试。无 UI，无真实 OS API。

### PR 2 — Agent Desktop PiP 壳

- 标题：Show Agent Desktop PiP for an active desktop session
- 影响：`ui/`、`ui-tests/`、`compose_icon`、Escape 栈
- 依赖：PR 1
- 内容：右下角 PiP、展开、暂停/接管/隐藏/停止的 UI 绑定到 fake runner。Playwright 覆盖 Escape 与隐藏≠停止。

### PR 3 — Windows ChildSession adapter

- 标题：Run Agent Desktop on a Windows child session
- 影响：`src-tauri` Windows 条件编译、文档
- 依赖：PR 1、PR 2
- 内容：真实 child session 生命周期、帧、输入；CI 仍走 fake。能力探测失败时 PiP 显示可行动错误。

### PR 4 — macOS native background adapter

- 标题：Run Agent Desktop via macOS accessibility and ScreenCaptureKit
- 影响：`src-tauri` macOS 条件编译
- 依赖：PR 1、PR 2
- 内容：后台路径、thumbnail 拒绝、capture 失败隔离、service single-flight。Escalation 只建模，不起 VM。

### PR 5 — Agent 工具

- 标题：Expose agent_desktop tools to the agent loop
- 影响：`crates/wisp-tools` 或 Tauri tool 注册、skills 文档、审批
- 依赖：PR 1，建议也依赖 PR 2
- 内容：status/start/stop/screenshot/act/pause。明确与 `web_*` 的分工。

### PR 6 — 平台 escalation 与 Linux backend（后续）

- 依赖：PR 4 的真实失败数据
- 内容：macOS isolated runtime；Linux 选定方案后的 adapter。不在本提案的 v0 验收内。
