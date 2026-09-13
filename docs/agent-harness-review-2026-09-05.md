# Agent Harness 研发方向审查

日期：2026-09-05。审查基线：`c786666cc44fba1d11978918ba76819b55d85e53` 及当前工作区。

这是源码、历史记录、评测配置和已有报告的架构审查，不是当前可执行程序的验收。没有修改运行时代码，没有运行 Rust 编译、完整测试或真实模型回放。工作区原有 DepMap 数据分析、服务和文档改动均保留。

**结论：通用 Harness 底座值得保留；最近的 DepMap 集成出现了方向偏移。**

偏移在于：逐渐用角色提示词和固定工作流规定模型的解题过程，用停止整轮执行处理本应分类恢复的错误，并通过流程断言巩固这些行为。不能据此否定整个项目，也不能把删除所有约束当成解决方案。科学证据、权限、资源和副作用边界仍然需要明确。

Harness 的目标应该是让模型在明确授权内自主完成任务，同时能获取真实状态、执行、观察、验证、恢复。判断约束是否合理，重点是它保护什么不变量、由什么机制执行、是否有评测支持，而不是提示词包含多少个 must。

**已有工作的价值**

| 能力 | 已检查的实现 | 判断 |
|---|---|---|
| Agent 工具循环、取消、重试、重复调用检测 | `crates/wisp-core/src/agent.rs` | 保留，是通用执行基础 |
| 历史归档、上下文压缩、持久 checkpoint | `crates/wisp-core/src/context.rs` | 保留，是长任务连续性的基础 |
| 项目会话恢复、工具调用配对修复 | `src-tauri/src/agent_turn.rs:701` | 保留，不应依赖角色记住恢复步骤 |
| 执行上下文、Run 预检、提交和恢复 | `src-tauri/src/run_context.rs:649`、`:968` | 保留，尤其适合远程和长时间科学计算 |
| 宿主拥有的能力及授权快照 | `crates/wisp-core/src/delegation_policy.rs:1` | 保留权限边界，避免把方法偏好混入权限 |
| 按需 Skill 搜索和加载 | `crates/wisp-core/src/system_prompt.rs:124` | 保留，减少无关上下文 |
| Reader 引用回溯、Reviewer 会话审查 | `src-tauri/src/project_reader.rs:18`、`src-tauri/src/review.rs:28` | 专门任务可以有严格输入输出契约 |
| CLI 轨迹和离线评测 | `crates/wisp-cli/src/eval.rs`、`docs/headless-agent-testing.md` | 保留，但要补齐与桌面生产装配的一致性 |

这些说明项目已经有相当多正确的 Harness 投入。研究工作台的 Project、ExecutionContext、Run、Artifact 等对象能提供真实状态；问题发生在把它们全部变成每次回答都必须经历的流程时。

**发现一：Specialist 混合了角色、权限、方法和调度职责。优先级高。**

`src-tauri/src/specialists.rs:48` 的 DepMap rubric 同时规定：Workflow 必须先启动、查询的首个工具、何时读取 Run、何时允许计算、R/Python 选择、文件目录、Skill 的阶段顺序、允许委派的任务、QC 和大量统计解释禁令。

角色原始结构则是 instructions、model、skills 和 connectors（`:178`）。Reader 的短而严格的只读检索契约，与面向开放研究问题的 DepMap 主 Agent 不是同一类任务；不应使用统一的“所有 Specialist 都要放开”策略。

建议让开放任务的 Specialist 只描述领域目标、默认偏好和质量标准。方法细节保留在按需 Skill；工具参数及数据语义放到工具边界；授权、资源与文件访问由宿主执行。不要只是把同一大段强制流程挪到 Skill 后继续全量注入。

**发现二：流程匹配变成强制审批入口，可恢复故障被提升为整轮终止。优先级高。**

`specialists.rs:53` 要求语义匹配 Workflow 时先调用 `start_workflow`，失败即停。`src-tauri/src/quick_actions.rs:557` 的工具描述也包含相同路由要求。

但 `start_workflow` 成功时仅创建 `awaiting_user_approval` 草稿（`:663`），并不执行。`workflow_blocked`（`:454`）调用 `stop_turn()`；启用状态保存、策略加载和草稿创建失败（`:621`、`:641`、`:668`）都进入该路径。这里讨论的是这些具体分支，不是声称所有工具错误都会终止 Agent。

这使正常研究请求受到已注册模板及其配置健康度的支配。历史审计 D17、D19 也记录了能力发现故障与失败后绕过边界的问题。

应保留禁止绕过用户拒绝和授权边界的能力，但区分 permission_denied、approval_required、invalid_arguments、temporary_unavailable 和 capability_unavailable。策略无法解析时禁止执行相关受控操作，同时允许解释、诊断或既有授权内的独立工作。Workflow 应是用户明确选用或模型有理由采用的执行方案；其存在本身不应强迫所有语义相近请求进入审批。

**发现三：提示词与执行契约已经出现具体冲突。优先级高。**

- `specialists.rs:97` 允许用户明确要求 Python 或缺乏可行 R 实现时用 Python；`depmap_agent.rs:549` 的校验器却只接受 manifest.language 为 R。符合前者的分析仍无法通过后者。应先明确产品是否支持例外，再让方法选择、schema、工具和验证一致。
- `specialists.rs:105` 要求新 Run 通过验证后解释；`:131` 又把数值来源限定为当前轮成功的 `depmap_evidence` 或 `depmap_query`。按字面执行会排除当前已验证 Run，也不利于跨会话复用有效证据。来源应绑定可解析的证据、Run、版本和产物，而不只是“本轮是否调用过两个查询工具”。
- 通用 system prompt 也混有许多具体产品路由、环境安装和图形工作流要求（`system_prompt.rs:89`、`:103`、`:111`）。因此只缩短 DepMap rubric 不足以清理责任边界。

**发现四：部分修复是正确的接口工程，部分则在积累失败个案禁令。优先级中高。**

`docs/depmap-agent-debug-audit.md` 中，癌种别名规范化、扁平 schema、显式 metric、紧凑证据、coverage gap 与查询失败区分，直接修复了工具可用性和观测质量，值得保留。

另一部分修复把每次错误写成新禁令，如均值/中位数不能证明双峰、某些亚组不能推断、特定词不能使用。其科学警惕性有价值，但开放式主 Agent 不应主要靠积累个案禁令学习统计推断。

优先完善已有证据返回结构里的 metric、scope、cohort、release、阈值和来源。领域 Skill 解释推断限制。评测和审查检验结论是否超出证据；不要试图把所有科学判断都变成程序硬编码或禁词。

**发现五：现有评测不能证明生产 Specialist 已被改善，而且存在可直接检查的错误判分。优先级最高。**

`crates/wisp-cli/src/eval.rs:1436` 的 build_agent 使用通用 registry、fixture 工具和通用 system prompt（`:1488`），没有装配桌面 `agent_turn.rs:729` 的 Specialist 指令。离线 scripted provider 的行为由脚本指定；它验证执行和判分链路，不能验证模型是否被复杂规则困住。live 模式能评估模型在该 fixture 环境中的行为，但仍不自动等价于桌面 DepMap Specialist。

此外，suite 中有明确的自相矛盾：

- `depmap-agent-v1.yaml:319` 的脚本答案说“不能据此提出机制、药物或合成致死结论”，`:322` 却禁止出现“合成致死”。
- `:347` 的脚本答案否定 ATF5-high 亚组及路线不可行判断，`:350` 却禁止这些子串。
- evaluator（`eval.rs:1630`）按包含关系判分，不理解否定。本次只读字符串检查确认了上述三个冲突，没有冒称执行完整 Rust suite。

已有 `target/depmap-eval/depmap-agent-offline-v1.json` 是 2026-08-30 的六例 Scripted 报告；当前 YAML 已有更多 case，不能用旧的 6/6 证明当前配置通过。`docs/depmap-debug-trajectory-test-report.md` 自己也明确区分了源码标记检查、Rust 未运行和新 EXE 未回放。

先修复量尺，再调整 Agent。保留协议测试；新增使用相同生产提示词和能力装配、替换外部 IO 的离线集成测试。真实模型比较应单独运行，固定模型、数据快照、工具与预算，记录配置版本。

评测同时衡量任务完成、证据正确、错误恢复、有效产物、额外确认次数、成本和延迟。合法的替代执行路径不应仅因工具顺序不同被判错。用户明确要求 query-only、确有授权依赖或数据依赖时，顺序和禁止工具断言仍然合理。

**发现六：run_validated 的保证范围应收窄表述并增强证据绑定。优先级中高。**

`depmap_agent.rs:391` 检查持久 Run 成功及退出码、文件存在、输出声明匹配，并读取三个 JSON。`:525` 检查 schema、语言、标识、状态、样本数基本关系和 QC 字段。

这是一层有用的执行与产物契约检查。但本函数没有独立重算统计，也不能仅凭分析自行写出的 qc.status=pass 证明混杂控制、因果推断或所有数值正确。它查询 artifacts，但没有在这些检查中验证当前读取文件与对应 Run 产物的内容哈希相等。这里不据此断言其他 Run 路径没有产物保护。

建议明确区分执行成功、产物契约有效、方法特定 QC 和科学审查。先利用已有 Run/Artifact 元数据补强对应关系；不急于引入新的总控框架。

**建议的职责分配**

| 层 | 应承担的职责 |
|---|---|
| 主 Agent | 理解目标、选择方法、规划、工具组合、诊断恢复和解释证据 |
| Specialist | 领域目标与偏好，不拥有授权判定和固定执行状态机 |
| Skill | 按需提供方法、示例、统计注意事项及领域参考 |
| 工具及数据服务 | 准确 schema、可理解结果、有限输出、结构化错误及来源 |
| Harness/运行时 | 授权、副作用、资源、取消、持久执行、上下文管理和恢复 |
| Workflow | 已选用的可复用执行方案，适合重复任务和明确交付合同 |
| 验证与评测 | 检查真实结果、来源、恢复能力和质量；输出可操作反馈 |

DepMap 的知识服务、R 参考实现和数据处理脚本可以作为领域包持续研发。当前 `agent_turn.rs:623` 按 Specialist ID 装配领域工具，`specialists.rs:285` 又按 ID 固定必需 Skill；这是领域集成与宿主耦合的具体位置。先在这些装配边界做最小调整，不必为了架构纯洁性移动所有文件或重建插件系统。

**建议的小步实施顺序**

1. 评测修复：消除否定句误判；让生产 Specialist 提示词/工具选择参与集成测试；建立当前版本基线。此阶段不改变用户权限。
2. 契约一致性：解决 R/Python 例外、数值证据来源和验证保证范围；加针对性回归。
3. 单一行为实验：选择 Workflow 故障分类或默认路由中的一个问题，允许授权内恢复，保留明确拒绝和不可绕过边界。比较任务完成与错误率。
4. Specialist 精简：每次移除一组重复流程规则，保留对应领域知识和工具语义，用相同固定及留出任务比较。不要同时更换模型、工具集、提示词和数据。
5. 根据评测决定后续投入：优先修复观测、执行、恢复和反馈缺口。没有证据支持时，暂停添加新的 Specialist 专属强制流程和角色数量。

可选的比较实验：同一模型和工具环境下，对比当前 Specialist、精简角色、通用 Agent 加按需领域 Skill。先保留相同权限与资源边界以隔离提示词影响，再单独实验 Workflow 路由调整。覆盖原始失败案例、未用于调优的癌种/基因、用户指定 Python、无需 Workflow 的查询、故障恢复、跨会话继续和明确授权的重计算。

**外部依据与适用限制**

- Anthropic，[Building effective agents](https://www.anthropic.com/engineering/building-effective-agents)：区分由代码预定路径的 workflow 和模型动态决策的 agent；从简单方案开始；重视工具接口设计。
- Anthropic，[Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)：通过环境、进度产物和验证支持跨上下文持续工作，而非只寄望一句自主执行指令。
- Anthropic，[Harness design for long-running application development](https://www.anthropic.com/engineering/harness-design-long-running-apps)：约束可验收交付、让执行者选择路径，并按实际模型表现增加或移除脚手架。

这些是工程案例，不是科学 Agent 的唯一标准，也不能直接证明减少 Wisp 规则后一定更好。本文关于方向偏移的判断来自当前源码；改善幅度必须通过上述对照评测确认。
