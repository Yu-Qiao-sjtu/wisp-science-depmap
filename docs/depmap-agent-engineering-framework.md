# DepMap Agent 工程编排框架

状态：目标架构（Agent-first），Phase 1 与 Evidence Ledger 最小闭环已实现  
适用基座：Wisp Science v1.8.x + DepMap 26Q1  
最后更新：2026-08-30

## 1. 文档目的

本文定义 Wisp Science 中 DepMap Agent 的工程边界、调用链、状态模型和实施顺序。
它回答的核心问题不是“怎样再增加一个固定 Workflow”，而是：

> 如何让用户通过自然语言与一个可信、可追溯、可扩展的科学 Agent 交互，
> 由 Agent 动态选择本地 MCP、Skills、子 Agent 和持久化 Run，同时避免把
> 182 GiB 预计算知识库、原始矩阵或未经验证的模型记忆直接放入上下文。

本文是 DepMap Agent 后续路由和 harness 开发的规范。已有 Workflow 基础设施继续
保留，但不再是普通 DepMap 问题的默认入口。

## 2. 产品定位

DepMap Agent 定义为：

> 本地优先、Agent-first、以可追溯科学证据为核心的科研智能体。

它不是以下任一单独组件：

- 不是 170/182 GiB 知识库本身；
- 不是一个固定的八节点 Workflow；
- 不是普通向量 RAG；
- 不是让模型自由生成 SQL、R 或统计结论的聊天机器人；
- 不是预先为所有基因生成自然语言“证据卡”的离线流水线。

其核心关系为：

```text
Agent       决定做什么、缺什么、下一步是什么
Specialist  规定领域身份、工具与科学边界
Skill       规定怎样正确完成一类科学任务
MCP         返回有界、结构化、可追溯的真实数据
Ledger      记录回答中的结论究竟来自哪条证据
Run         承载新的、可复现的统计计算
Workflow    仅承载长时间、多阶段、需恢复或独立评审的任务
```

## 3. 已有基础

### 3.1 本地 DepMap MCP

当前本地服务位于 `services/depmap_mcp/`，默认 HTTP 端点为：

```text
http://127.0.0.1:8877/mcp
```

它只读访问现有 DepMap 26Q1 预计算库及可选 TCGA 表达/生存桥，不复制大型数据，
不建立 SSH，不启动新分析。当前暴露九个工具：

1. `depmap_status`
2. `depmap_resolve_lineage`
3. `depmap_lineage_catalog`
4. `depmap_lineage_dependencies`
5. `depmap_lineage_direction_discovery`
6. `depmap_gene_evidence`
7. `tcga_gene_expression_survival`
8. `depmap_pair_evidence`
9. `depmap_drug_evidence`

每次成功响应使用 Evidence Envelope，至少保留：

- 确定性的 `evidence_id`；
- 数据发布版本；
- 规范化后的请求；
- 统计指标语义；
- `FOUND`、`NOT_RETAINED`、`INELIGIBLE`、`NOT_COMPUTED`、
  `MODULE_UNAVAILABLE` 等覆盖状态；
- 规范化来源和限制；
- DepMap 细胞系证据与 TCGA 患者证据的明确分离。

### 3.2 Wisp 已有能力

| 层 | 当前实现位置 | 已有能力 |
|---|---|---|
| Agent loop | `crates/wisp-core/` | 模型调用、工具循环、上下文管理 |
| 对话入口 | `src-tauri/src/agent_turn.rs` | 会话、工具注册、流式事件 |
| DepMap 适配 | `src-tauri/src/depmap_agent.rs` | 本地/远程提供者、有界查询、证据压缩 |
| Specialist | `src-tauri/src/specialists.rs` | DepMap 身份、领域指令与资源边界 |
| Skills | `skills/depmap-*` | 查询规范、新计算规范、科学解释规则 |
| MCP 客户端 | `crates/wisp-mcp/` | stdio/HTTP、工具发现与调用 |
| 子 Agent | `src-tauri/src/delegation_runtime.rs` | 权限、预算、依赖、取消、重试 |
| Workflow | `src-tauri/src/quick_actions.rs` | 注册模板、审批、持久化 DAG |
| 持久化 | `crates/wisp-store/` | 项目、会话、Run、Artifact 等状态 |
| UI | `ui/` | 对话、Agents 面板、审批和进度 |

## 4. 参考工程与采用原则

### 4.1 BioMCP

[BioMCP](https://github.com/genomoncology/biomcp) 使用统一的
`discover/search -> get -> pivot -> batch` 数据语法。MCP 是稳定数据平面，Skill 是
可选调查指南，Agent 不需要为每种问题先建立固定 Workflow。

采用：统一实体解析、先发现后聚焦、有界结果、跨实体 pivot。  
不照搬：依赖在线多数据源的具体实现。

### 4.2 Data Commons MCP

[Data Commons MCP](https://docs.datacommons.org/mcp/index.html) 将交互分为能力搜索、
元数据确认和观测值获取，并把调查 playbook 作为 MCP resources/skills 提供。

采用：先回答“有什么、是否可查、覆盖到哪里”，再请求具体数值。  
不照搬：地理实体和统计变量本体。

### 4.3 ToolUniverse

[ToolUniverse](https://github.com/mims-harvard/ToolUniverse) 使用声明式工具元数据、
动态工具发现、Skills、两级缓存、工具指纹、单工具并发限制和流式工具输出。

采用：工具目录检索、缓存指纹、并发治理、结构化进度。  
不照搬：把所有数据重新制作成通用向量库；DepMap 数值查询仍应使用确定性索引。

### 4.4 Biomni

[Biomni](https://github.com/snap-stanford/Biomni) 以 ReAct Agent 为中心，通过检索选择
相关工具、数据库和软件；声明式工具描述与实际函数分离。

采用：Agent 动态规划和按需资源选择。  
不照搬：允许宽泛系统命令和大规模科学环境直接暴露给普通查询。

### 4.5 Vanna 与 DB-GPT

[Vanna](https://github.com/vanna-ai/vanna) 强调用户身份贯穿工具权限，并把进度、表格、
图和摘要实时流式返回。[DB-GPT](https://github.com/eosphoros-ai/DB-GPT) 将 Agent 的
自主计划、SQL/代码工具、Skills 与复杂 Workflow 分层。

采用：用户级权限、结构化 UI 事件、Agent-first 与 Workflow escalation。  
不照搬：自由 Text-to-SQL；DepMap 不允许模型直接生成对大型存储的任意查询。

## 5. 目标总体架构

```text
┌────────────────────────────────────────────────────────────┐
│ Wisp Science 交互层                                        │
│ 对话 · 追问 · 证据引用 · 数据卡片 · 进度 · 审批 · Artifact │
└───────────────────────────┬────────────────────────────────┘
                            │ user request / activity events
┌───────────────────────────▼────────────────────────────────┐
│ DepMap Main Agent                                           │
│ Intent Router · Entity Resolver · Planner · Claim Composer │
│ Coverage Reasoner · Escalation Policy                       │
└──────────┬────────────────┬───────────────────┬────────────┘
           │                │                   │
     Specialists          Skills        Execution Policy
           └────────────────┴───────────────────┘
                            │ typed tool calls
┌───────────────────────────▼────────────────────────────────┐
│ Scientific Tool Plane                                       │
│ Local DepMap MCP · TCGA MCP · Literature · Project/Run     │
└───────────────────────────┬────────────────────────────────┘
                            │ evidence envelopes
┌───────────────────────────▼────────────────────────────────┐
│ Scientific State Plane                                      │
│ Evidence Ledger · Cache · Run · Artifact · Paper · Decision│
└───────────────────────────┬────────────────────────────────┘
                            │ bounded indexed reads
┌───────────────────────────▼────────────────────────────────┐
│ DepMap 26Q1 Knowledge                                       │
│ Core/Full indexes · Parquet/RDS · Manifest · QA            │
└────────────────────────────────────────────────────────────┘
```

## 6. Agent 主循环

主 Agent 每轮使用以下状态机，而不是先查找一个固定 Workflow：

```text
receive_request
  -> classify_intent
  -> extract_entities
  -> resolve_scope
  -> inspect_capability_if_needed
  -> select_smallest_tool_set
  -> execute_bounded_query
  -> register_evidence
  -> assess_sufficiency
       |-- sufficient -> compose_grounded_answer
       |-- ambiguous  -> ask_user
       |-- follow-up  -> one targeted query -> reassess
       |-- long task  -> propose escalation
       `-- new stats  -> propose persisted Run
```

### 6.1 意图类型

建议在 DTO/store 中使用明确枚举，而不是只依赖系统提示词：

```text
provider_status
lineage_resolution
cancer_inventory
cancer_direction_discovery
gene_evidence
gene_pair_evidence
drug_gene_evidence
evidence_comparison
result_interpretation
topic_exploration
literature_validation
new_analysis
report_generation
```

路由输出应包含：

```json
{
  "intent": "gene_evidence",
  "entities": {
    "gene": "ESR1",
    "cancer_term": "乳腺癌",
    "canonical_lineage": "Breast"
  },
  "missing": [],
  "ambiguities": [],
  "execution_level": "L1_DIRECT",
  "reason": "single gene plus resolved cancer; bounded read-only evidence"
}
```

该对象是可观测的路由决定，不是科学证据，也不应被当成最终结论。

### 6.2 实体解析

- 基因符号必须规范化，但不得猜测不存在的 anchor gene；
- 癌种词先调用 `depmap_resolve_lineage`；
- `RESOLVED` 可以继续；
- `AMBIGUOUS` 必须向用户展示候选项；
- `PROPOSED` 必须经用户确认；
- DepMap lineage 是模型分组代理，不等于临床组织学诊断；
- 用户的原始疾病表述与规范化 lineage 必须同时保留。

### 6.3 最小工具集原则

Agent 每次只获得当前意图所需工具：

- 癌种查询：resolver、catalog、direction discovery；
- 单基因查询：resolver、gene evidence；
- 基因对：resolver、pair evidence；
- 药物问题：resolver、drug evidence；
- 新分析：先读取 coverage，再获得 Run/代码能力；
- 文献任务：只在用户问题需要外部创新性或临床论证时启用。

不得把所有 MCP、写文件、浏览器、shell、R 和 Workflow 工具同时交给每一轮模型。

## 7. 四级执行策略

| 级别 | 典型请求 | 运行方式 | 默认审批 |
|---|---|---|---|
| L1 Direct | 单基因/癌种/药物/基因对查询 | 主 Agent 调一次有界 MCP | 不需要 |
| L2 Investigate | 跨模块比较、癌种方向发现、结果解释 | 主 Agent 动态执行少量查询并多轮追问 | 不需要 |
| L3 Delegate | 文献创新性、独立可行性或临床评审 | 一个或少量子 Agent，输入为紧凑证据包 | 启动前批准 |
| L4 Durable | 新统计、长任务、图文报告、多阶段可恢复工作 | Persisted Run 或 Workflow | 必须批准 |

执行层级由任务性质决定，不使用“必须 5 次”“最多 8 次”之类固定工具调用数作为科学
完整性的判断。通用预算和超时仍然是安全上限，但停止条件应是：证据已足够、边界已报告、
用户需要澄清，或继续执行必须升级权限。

### 7.1 L1 示例：基因加癌种

```text
用户：ESR1 在乳腺癌中有什么 DepMap 证据？
Agent：extract ESR1 / 乳腺癌
  -> depmap_resolve_lineage("乳腺癌")
  -> selected Breast
  -> depmap_gene_evidence(gene=ESR1, lineage=Breast)
  -> register evidence_id
  -> 分开陈述观测、解释、覆盖缺口和下一步
```

### 7.2 L2 示例：不指定基因的癌种方向

```text
用户：结肠癌基于当前数据可以做哪些方向？
Agent：不得虚构 APC/KRAS anchor
  -> resolve 结肠癌 -> Bowel
  -> lineage_catalog(Bowel)
  -> lineage_direction_discovery(Bowel)
  -> 根据返回 family shortlists 提议方向
  -> 用户选择方向后才查询具体 gene/pair/drug
```

### 7.3 L3/L4 升级

当用户要求“检索全部相关论文并独立论证创新性、可行性和临床转化性”时，Agent 先
展示升级草案：目标、子任务、外部服务、预算和预期输出。用户批准后才启动子 Agent。

当用户要求“生成可复制到论文的图、Results 和 Methods”时，必须升级为 L4，冻结证据、
生成可追溯 Artifact，并进行输出 QA。

## 8. Evidence Ledger

### 8.1 目的

MCP 已经返回 `evidence_id`，但模型在多轮对话中还需要一个持久化、可查询的证据账本。
Ledger 不预先生成全部自然语言卡片，而是在证据实际被调用和使用时登记。

### 8.2 建议数据模型

```text
EvidenceRecord
  id
  project_id
  frame_id
  provider
  provider_version
  tool_name
  canonical_arguments_json
  evidence_id
  evidence_status
  metric_semantics_json
  provenance_json
  payload_ref / compact_payload
  created_at

ClaimRecord
  id
  project_id
  frame_id
  text
  claim_type: observation | interpretation | hypothesis | limitation
  evidence_record_ids[]
  contradiction_ids[]
  answer_message_id
  created_at
```

### 8.3 强制规则

- 每个数值型科学陈述必须引用当前 Provider 的 EvidenceRecord；
- 文献机制、治疗、创新性和临床陈述必须引用 Paper/Publication Evidence；
- 模型记忆只能生成待验证假设，不能生成 observation；
- 同一 `evidence_id` 在同一版本中可复用，不重复读取大型存储；
- Provider 版本变化时，不得静默复用旧证据；
- 相互矛盾的证据不得覆盖，必须并列记录；
- Ledger 保存“用了什么”，不保存模型的隐藏推理过程。

## 9. 缓存与性能

缓存键建议为：

```text
provider_id
+ provider_release
+ server_version
+ tool_name
+ canonical_arguments
+ result_contract_version
```

采用两级缓存：

1. 会话内存 LRU：重复追问即时返回；
2. SQLite 持久化：跨会话复用 Evidence Envelope。

缓存只适用于确定性、只读调用。时间敏感文献结果使用 TTL；DepMap 26Q1 静态结果可保持到
Provider release 或结果契约变化。并发相同请求应 single-flight 合并，避免多个 Agent 同时
扫描同一 Parquet 分块。

## 10. 进度与 UI 事件

Agent 活动应流式返回可验证的执行状态，不展示隐藏 chain-of-thought。建议事件契约：

```json
{
  "kind": "scientific_activity",
  "phase": "querying_evidence",
  "tool": "depmap_gene_evidence",
  "scope": {"gene": "ESR1", "lineage": "Breast"},
  "status": "running",
  "elapsed_seconds": 3,
  "evidence_received": 0,
  "coverage_gaps": 0,
  "message": "正在读取 ESR1 × Breast 的有界预计算证据"
}
```

阶段至少包括：

```text
understanding_request
resolving_entities
checking_coverage
querying_evidence
registering_evidence
comparing_results
waiting_for_user
proposing_escalation
completed
failed
```

直接 MCP 查询的进度显示在主对话；只有真正启动子 Agent/Workflow 时才自动打开右侧
Agents 面板。

## 11. 审批与权限

### 不需要逐次批准

- loopback、本地、只读 DepMap MCP；
- 已授权项目中的有界查询；
- Evidence Ledger 登记；
- 读取已有 Run/Artifact 元数据。

### 必须批准

- 外部网络文献检索达到委托/长任务级别；
- 新建统计 Run；
- 写入项目文件、生成正式报告或图片；
- 启动子 Agent；
- 使用 SSH/GPU/调度器；
- 修改、发布或上传数据和 Artifact。

审批必须发生在副作用或长任务开始前，而不是在主 Agent 理解用户问题之前。

## 12. Workflow 的正确边界

Workflow 是 Agent 的一种执行工具，只在以下条件之一成立时使用：

- 用户明确请求注册 Workflow；
- 任务需要多个独立角色评审；
- 任务需要跨进程恢复、取消和重试；
- 任务产生多个正式 Artifact；
- 执行时间或外部服务成本明显高于普通查询；
- 统计计算必须经过 Run/QA 状态机。

不得仅因为用户的自然语言“语义上像某个模板”就强制 `start_workflow`。正确交互为：

```text
Agent 先理解和查询
  -> 发现需要长任务
  -> 向用户展示 Workflow/Run 草案
  -> 用户批准
  -> 执行
```

现有 `depmap_gene_to_cancer_topics` 和 `depmap_selected_topic_report` 可保留为经过验证的
L3/L4 模板，但普通数据查询和初步课题讨论不应自动进入它们。

## 13. 新计算边界

覆盖缺口不自动授权新分析。Agent 必须按以下状态转换：

```text
coverage_gap
  -> explain_gap
  -> identify_computation_capability
  -> propose_analysis_spec
  -> user_authorization
  -> persisted_run
  -> native_success
  -> qc_passed
  -> validated_evidence
  -> ledger_registration
  -> interpretation
```

任何新的矩阵、统计检验、FDR、模型、分组或图都属于新计算，不得描述为“纯查询”。

## 14. 当前差距

| 能力 | 当前状态 | 后续动作 |
|---|---|---|
| 本地只读 MCP | 已有 | 保持小工具面和证据契约 |
| 中文癌种解析 | 已有基础 | 用真实轨迹扩充歧义测试 |
| DepMap Specialist | Agent-first 已接入 | 继续用真实轨迹收紧边界 |
| Skills | 已有 | 减少互相冲突和过度触发 |
| 注册 Workflow | 已降级为 escalation | 仅显式请求或 L4 durable 路由 |
| 子 Agent进度 | 已修复基础 | 继续加入结构化科学阶段 |
| 显式 Intent Router | 已有最小闭环 | 后续增加活动时间线 DTO/UI |
| Evidence Ledger | 已有账本与最终回答硬门 | 后续增加文献/TCGA统一 ClaimRecord 和证据详情 UI |
| MCP 结果缓存 | 部分依赖底层 | 增加 release-aware 两级缓存 |
| Agent 路由评测 | 部分轨迹测试 | 建立黄金场景与自动评分 |

## 15. 实施顺序

### Phase 1：Agent-first Router

已完成最小实现：

1. `depmap_agent_route` 以封闭 schema 接收意图和实体；
2. 主机验证必需实体，并返回 L1/L2/L3/L4、允许的下一工具和审批要求；
3. 注册 Workflow 的语义强制优先规则已改为显式请求或 L4 才升级；
4. 旧的 pre-turn `intent_router` 不再把基因、癌种或课题请求注入固定 Workflow；显式 Workflow 名称仍由独立解析器处理；
5. 已覆盖基因无癌种、癌种无基因、基因加癌种、缺失实体和显式 Workflow 测试。

待补：把路由决定显示为独立的科学活动时间线，而不只显示为普通工具活动。

### Phase 2：Evidence Ledger

已完成最小实现：

1. SQLite migration `0055_scientific_evidence_ledger`；
2. `depmap_query` 与 `depmap_evidence` 成功后自动登记稳定 `evidence_id`；
3. 工具结果返回 `evidence_ref`，记录 provider、release、规范化参数、语义、来源和紧凑 payload；
4. `depmap_evidence_history` 支持列出本会话证据或按 id 恢复；
5. 超过 256 KiB 的账本 payload 只保留元数据、字节数和 SHA-256，不把大结果复制进 SQLite。

已完成 DepMap 最终回答 Grounding Gate：

6. DepMap Specialist 用宿主受控实现替换通用 `attempt_completion`，普通 Agent 不受影响；
7. 每条数据声明提交 `evidence_id + JSON Pointer + exact claim`，并在正文中带 `[E#]`；
8. 宿主只接受当前轮次写入或刷新的证据，拒绝跨轮陈旧证据；
9. 数值按声明显示精度与 Pointer 指向的 JSON 数值核对，允许正常四舍五入但拒绝无来源数值；
10. `coverage_gap`、`blocked`、`validation_failed` 不得作为阳性证据；绑定结果中的
    `NOT_RETAINED`、`INELIGIBLE`、`NOT_COMPUTED`、`MODULE_UNAVAILABLE` 必须在回答中显式披露；
11. 未限定的因果、已验证合成致死和临床疗效声明会在答案发布前被拒绝；
12. 验证成功后自动附加可见的机器核验证据索引；
13. Agent 内核按 DepMap 会话声明强制使用完成工具：自由文本草稿既不流式展示也不持久化，
    会被回送给模型补查证据并重新提交，因此模型不能通过省略工具调用绕过核验；达到迭代上限
    时失败关闭，不生成未核验的兜底总结；
14. TCGA 表达/生存结果已通过现有 DepMap 查询入口写入同一账本；通过
    `depmap_validate_run` 的 manifest/result/QC 以 `run_validated` 登记并可按 Pointer 核验；
15. 明确声明 `literature_search` 能力的后台检索结果按 Workflow、delivery 和子工具轨迹登记。
    有 DOI/PMID/URL/paper id/reference 的记录标为 `literature_retrieval`，无定位符的记录标为
    `literature_unverified` 并禁止作为阳性依据。当前文献门验证可追溯性，不宣称自动完成语义蕴含判断。

待补：持久化 ClaimRecord、文献 claim-to-passage 蕴含验证器、证据详情 UI、跨会话
release-aware 复用。超过 256 KiB
而被哈希化的结果不能直接支持数值声明；Agent 必须先执行更窄的查询取得可定位字段。

### Phase 3：缓存与工具发现

1. canonical argument fingerprint；
2. memory + SQLite cache；
3. single-flight 和每工具并发上限；
4. 根据意图只注入相关工具；
5. 记录 cache hit、耗时和 payload 大小。

### Phase 4：动态调查与进度

1. 主 Agent 小步查询并在每步后判断 sufficiency；
2. 增加 `scientific_activity` 事件；
3. 支持中途追问和用户改写研究范围；
4. 不用固定调用次数替代证据饱和；
5. 长任务才切换 Agents 面板。

### Phase 5：Escalation 与正式报告

1. 子 Agent接收紧凑 Evidence Ledger 引用，而非整个聊天历史；
2. 用户审批长文献、评审和新计算；
3. 报告 Workflow 冻结证据版本；
4. 图、Results、Methods 和图注引用同一 Evidence/Run；
5. Artifact 完成前进行文件、统计和引用 QA。

## 16. 验收场景

最低回归矩阵：

| 输入 | 预期路由 | 禁止行为 |
|---|---|---|
| `TP53 有什么数据？` | L1 gene evidence | 不虚构癌种 |
| `结肠癌有哪些方向？` | L2 Bowel discovery | 不虚构 APC anchor |
| `ESR1 在乳腺癌有什么证据？` | L1 gene + lineage | 不启动 Workflow |
| `KRAS 和 RAF1 在肺癌是否共依赖？` | L1 pair | 不混淆相关与差值 |
| `白血病中的 TP53` | ask user | 不静默选择 Myeloid/Lymphoid |
| `为 PTK7×肝癌做完整创新性评审` | propose L3 | 未批准不检索长文献 |
| `生成英文图文论文报告` | propose L4 | 未冻结证据不写 Results |
| `这个分析库里没有，重新算` | propose Run | coverage gap 不自动执行 |

每个场景至少评估：

- 路由正确性；
- 工具选择精度；
- 参数和实体规范化；
- 数值/文献证据忠实度；
- coverage state 解释；
- 是否发生不必要 Workflow；
- 是否发生未经批准的副作用；
- 首个可见进度时间；
- 总工具调用、缓存命中和上下文体积；
- 最终回答能否从 Claim 追溯到 Evidence。

## 17. 禁止的反模式

- 把 182 GiB 文件目录交给模型遍历；
- 为每个问题固定启动八节点 Workflow；
- 用证据卡预生成替代实时结构化查询；
- 让模型自由编写底层 Parquet/SQL/R 查询后直接报告结果；
- 把 `NOT_RETAINED` 或 `INELIGIBLE` 说成生物学阴性；
- 把 DepMap correlation、mutation/CNV mean difference 和 PRISM AUC 混为同一指标；
- 在没有 Paper evidence 时声称文献创新性或临床事实；
- 将 Skills 的说明文字作为证据；
- 用固定工具调用次数表示“最佳”或“完整”；
- Workflow 失败后由主 Agent 手工重建同一任务；
- 在用户未批准时启动新计算、写报告或访问远程服务器。

## 18. 最终工程定义

```text
Wisp DepMap Scientific Agent
= Conversational Main Agent
+ Typed Intent Router
+ Domain Specialist
+ Hot-loadable Skills
+ Bounded Local/Remote MCP
+ Evidence Ledger
+ Release-aware Cache
+ Optional Child Agents
+ Durable Run/Workflow Escalation
+ Approval, Provenance, Progress and Evaluation
```

在该框架中，Agent 是用户持续交互的主体；MCP 是可信数据接口；Skill 是操作规范；
Ledger 是科学可信度的连接层；Run 和 Workflow 是必要时才启用的执行基础设施。
