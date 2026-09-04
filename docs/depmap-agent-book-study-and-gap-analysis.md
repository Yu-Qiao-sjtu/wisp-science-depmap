# 从《AI Agent 构建指南》到 DepMap Agent：完整学习、架构映射与缺口分析

> 文档状态：架构研究与实施基线
> 参考仓库：<https://github.com/bojieli/ai-agent-book/tree/main/book>
> 阅读基线 commit：`2c9dc5fb8d142e9bef63b6690fcb853477616e81`
> 对照工程：Wisp Science v1.8.x 基座上的 DepMap/TCGA Agent
> 说明：本文讨论的是 Agent 工程设计，不把约 180 GiB 的预计算文件体积等同于 Agent 能力，也不把“文件存在”视为“已经可查询”。

## 1. 为什么要写这份文档

当前项目已经完成了大量 DepMap 26Q1 预计算，并逐步建立了本地/远程只读查询、MCP、Skill、专用 Agent、工作流、证据账本和轨迹评测。然而真实测试仍反复出现同一类现象：

- 用户用自然语言说“我做肝癌，想研究肿瘤细胞干性和转录因子”，模型能理解每个词，却只调用一个转录因子富集模块；
- 数据存储中有很多分析结果，但模型看不到完整的能力边界，因而退回常识或错误地报告 `not found`；
- 一个 `limit=20` 的展示上限被误解为分析范围只有 20 条；
- 一个稀疏结果中没有保留某行，被误述为“没有生物学关系”；
- 文献检索、候选课题、独立评审被做成较长 DAG 后，前置节点超时，后续节点全部阻塞；
- Debug 轨迹能够暴露问题，却还没有完全形成自动回归、失败归因和版本化更新闭环。

这些不是“模型不够聪明”或“数据量太大”能概括的问题。它们属于 **Agent Harness 没有把用户语义、知识能力、工具调用、统计语义和证据结论连成一个可验证闭环**。

本文有两个目标：

1. 按章节完整吸收《AI Agent 构建指南》的工程逻辑，而不是摘录几条口号；
2. 把书中的方法转译成 DepMap Agent 的目标架构、缺口清单、数据契约、开发顺序和验收标准。

---

## 2. 全书的统一主线

全书最基础的公式是：

```text
Agent = LLM + Context + Tools
```

它不是说把一个模型、一个长 Prompt 和若干函数拼在一起就结束了。其生产化展开是：

```text
Agent = Model + Harness

Harness =
  上下文管理
  + 工具接口
  + 确定性约束
  + 结果验证
  + 错误纠正与恢复
```

模型负责处理开放语义、提出候选解释和生成自然语言；Harness 负责决定模型能观察什么、可以做什么、参数是否合法、结果意味着什么、失败后如何恢复、何时必须停止以及哪些结论可以交付。

贯穿全书的五个模式尤其适合本项目：

1. **提议者—审核者**：生成与验证分离，审核者看产物和证据，不依赖生成者的自述；
2. **渐进式披露**：先暴露能力目录，再按需加载 Schema、Skill 和数据明细；
3. **只增不改**：运行轨迹、证据引用和状态事件尽量追加，便于缓存、重放和审计；
4. **边界集 + 保留集**：修复目标问题的同时，证明旧能力没有退化；
5. **最小 diff + 可回滚**：每次只改变一个可归因机制，避免在模型、Prompt、工具和数据层同时修改后无法判断原因。

这套思想直接否定了两种极端方案：一是把 180 GiB 数据直接塞给模型；二是用越来越长的提示词要求模型“记得调用所有数据”。正确方向是让大数据停留在数据层，让模型看见一个准确、可发现、可执行、可验证的能力视图。

---

## 3. 逐章学习与 DepMap 映射

### 3.0 引言：全书不是框架选型手册

[引言](https://github.com/bojieli/ai-agent-book/blob/main/book/introduction.md)把全书分成构建、评估与进化、协作三部分。其重点不是推荐某个流行框架，而是寻找能穿越模型迭代周期的原则：模型会升级，API 和产品会变化，但观察空间、动作空间、上下文组织、工具契约、验证反馈和安全边界始终存在。因此，本项目也不应把 DepMap Agent 绑定在某个模型的特殊提示技巧上；注册表、Schema、Evidence Ledger、Run 和评测应当在更换 GLM、GPT 或其他模型后继续成立。

### 3.1 第一章：Agent、观察/动作空间与 Harness

[第一章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter1.md)首先把 Agent 解释为大脑、眼睛和手脚：LLM 是决策核心，上下文决定它能看见什么，工具决定它能对环境做什么。对 Agent 而言，没有通过观察通道进入上下文的信息等同于不存在；没有通过动作接口开放的能力，即使模型知道方法也只能给出文字建议。

这一章的重要含义不是“多加工具”，而是重新定义观察空间和动作空间：

- 观察空间包含用户原话、实体解析结果、知识库能力目录、运行状态、工具返回、证据来源和错误信息；
- 动作空间包含只读查询、实体消歧、规划、持久运行、文献检索、人工确认和最终回答；
- ReAct 循环只有在每次动作后获得真实观察，才会闭合；
- Harness 的价值在于让 Agent **可靠地完成**，不是偶尔做对。

书中对 Workflow 与自主 Agent 的区分也很关键：固定、重复、高风险、容易验证的步骤适合 Workflow；路径依赖用户语义和中间结果的开放研究任务适合自主 Agent；生产系统往往混合二者。

对 DepMap 的映射：

- “查一个基因在一个癌种的已有证据”应走短路径 Agent + 有界查询，不应强制启动长 Workflow；
- “生成论文级课题报告并经过文献创新性、统计可行性、临床转化性三路评审”适合持久化 Workflow；
- “知识库没有预计算该表型，允许启动新统计分析”应升级为有审批的 Run，而不是让查询工具偷偷计算；
- 数据库支持什么、结果是否稀疏、某癌种是否合格，都必须成为观察，而不能藏在目录或 Prompt 里。

### 3.2 第二章：上下文工程、Skills、状态栏与压缩

[第二章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter2.md)说明上下文工程的核心是显式管理信息。上下文不是越长越好；无关信息会稀释注意力，动态前缀会破坏缓存，散落在历史中的事实会让模型反复检索甚至遗忘。

本章把 Skill 设计成三级渐进式披露：

1. 元数据目录常驻，让模型知道有哪些领域能力；
2. 选中后才加载 `SKILL.md` 核心流程；
3. 需要时再读取参考资料、模板和脚本。

Skill 和工具不是同一层：Skill 说明“怎样完成一类任务”，工具提供“实际可执行的动作”。如果能力发现只靠 Skill 描述，却没有对应工具或数据契约，Agent 仍然无法执行；如果工具很多但没有能力目录，模型也难以正确选择。

状态栏是另一个关键思想。任务目标、已调用工具、当前阶段、失败次数、剩余预算、等待中的依赖和最后真实进展，应由代码维护并放到轨迹末尾。模型会高度信任状态栏，所以状态不能由模型凭印象统计。对长任务，状态栏既服务模型，也服务用户界面。

压缩的重点不是简单删字，而是把大体积工具结果放到外部存储，只给模型相关摘要、稳定引用和可回取索引。压缩必须保留：用户目标、约束、关键决定、失败、来源和未解决问题。

对 DepMap 的映射：

- 180 GiB 数据不能进入上下文；模块目录、适用实体、统计语义和查询入口应形成小型 capability catalog；
- 每次查询结果应写入 Evidence Ledger，模型只接收有界 `EvidenceBundle`；
- `limit=20` 必须标注为 `returned_rows`，同时返回 `total_retained_rows`、`has_more`、`next_cursor` 和完整分析范围；
- 文献节点运行 600 秒时，UI 不能只显示“运行中”；应显示阶段、已完成查询数、最近活动、预算、可取消状态和是否等待外部服务；
- Skill 不应硬编码成唯一入口。用户显式指令、模型自主选择、Agent 规划器选择都可以加载同一 Skill，但能力和权限由 Harness 决定。

### 3.3 第三章：记忆、RAG、结构化知识与更新

[第三章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter3.md)区分用户记忆与共享知识库，并将 RAG 从“向量检索几段文字”扩展到结构化索引、文件系统范式、Agentic RAG 和知识更新治理。

对本项目最重要的是：不同问题适合不同索引。语义相似检索擅长从非结构化文档中找近义内容；精确基因名、癌种、分析模块、统计字段、版本和结果状态更适合结构化索引与符号过滤。知识库不仅要保存内容，还要保存来源、时间、冲突、适用范围和更新版本。

Agentic RAG 的含义也不是“让模型多搜几次”，而是把检索作为工具：Agent先理解问题，再选择检索源、过滤条件和深度，并根据结果决定是否继续。结构化概览常驻，原始细节按需召回。

对 DepMap 的映射：

- 基因、癌种、药物、通路、表型和分析模块不应只放进向量库；它们需要规范实体表和可验证 ID；
- 模块能力必须有结构化 Schema：输入角色、实体集合、癌种范围、样本阈值、统计量、多重检验、保留规则、查询入口和允许结论；
- 向量/语义检索可用于把“肿瘤起始细胞”“干性样细胞”“自我更新”映射到候选概念，但候选必须由维护词表和能力图验证；
- 结果行不能脱离 Manifest。否则模型无法区分“没有算”“样本不够”“算了但未保留”“模块不可用”和“确有支持证据”；
- 预计算模块更新必须版本化、可审查、可回滚，而不是替换目录后让旧对话失去可追溯性。

### 3.4 第四章：工具、MCP、主动发现与参数保真

[第四章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter4.md)把能力表达分为专用工具、通用执行器和 Skill。专用工具适合高频、参数复杂、权限敏感或跨平台差异明显的能力；通用执行器更灵活；Skill 用于按需传递方法和经验。

MCP 解决工具接入标准化，但不自动解决语义规划。即使所有模块都发布为 MCP 工具，如果模型一次看到几十个相似工具、描述不清、参数 Schema 与实际执行不一致，工具调用仍会失败。书中给出的方向是层次化组织、按需加载和主动工具发现，把“在几百个工具中猜一个”变成“先查询能力目录，再加载少量候选”。

参数保真尤其重要：模型看到的参数、Harness 校验的参数和后端执行的参数必须一致。不能让前端 Schema 说 `lineage` 可选，而后端在某模式下暗中要求它；也不能让默认 `limit`、筛选阈值或癌种映射只存在于实现细节。

对 DepMap 的映射：

- MCP 是数据边界，不是完整 Agent；
- `depmap_query`、`depmap_evidence` 等是动作接口，`capability_catalog` 是发现接口，`ScientificIntent` 与 `EvidencePlan` 才是语义桥；
- 不应把 27 个模块同时平铺进模型工具列表；先用能力检索缩小到 3–8 个候选，再暴露精确调用契约；
- 查询工具要返回机器可判定的状态枚举和证据引用，不能只返回一段自然语言；
- 对复杂研究意图，应该允许一次计划联动多个分块/模块，而不是一次调用一个分块、让模型自行拼接。

### 3.5 第五章：Coding Agent、形式化与错误恢复

[第五章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter5.md)指出 Coding Agent 成熟，不只是因为模型会写代码，而是软件工程已有类型系统、测试、版本控制、静态检查、运行时错误和可重放环境。代码是 Agent 的元能力：可以用于精确计算、表达规则、适配格式、生成图表/UI，并在覆盖缺口出现时创建新工具。

但代码能力不应成为绕过正式接口的捷径。生产 Harness 要对错误分类：参数错误、工具错误、上下文错误、控制流错误；区分可重试、需修正、不可重试和需人工接管；检测重复调用指纹；设置 Watchdog、预算和终止条件。

对 DepMap 的映射：

- 现有预计算结果必须优先走只读结构化查询；
- 只有明确 `NOT_COMPUTED` 且用户允许新分析时，Coding Agent 才生成 R/Python 脚本；
- 新分析必须成为持久 Run，包含输入、参数、代码、日志、Manifest、QA 和产物；
- 统计阈值、癌种资格、多重检验、结果状态转换应由代码强制，而不是写在提示词中；
- GLM-5.3 的过度思考、重复反向查询、600 秒无输出属于控制流与可观测性问题，应由 Harness 预算、阶段状态、重复检测和收口机制解决，而不是在 Skill 里写死“最多 8 次”。

### 3.6 第六章：异步、事件驱动与用户交互

[第六章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter6.md)把同步轮次扩展到异步事件：世界可以主动推送工具结果、任务进度和用户消息；长任务先发起，之后由事件收尾。快慢分离、取消、抢占、安全点和状态确认贯穿语音、Computer Use 和机器人场景。

对 Wisp Science 的直接启示是：后台 Agent/Workflow 不能把十分钟任务伪装成一个同步模型调用。运行过程应建模为事件流：

```text
queued -> resolving_intent -> discovering_capabilities -> querying
       -> aggregating -> validating -> synthesizing -> completed
                                      \-> needs_input
                                      \-> failed/retryable
                                      \-> cancelled
```

每个阶段都应有真实事件、时间戳、进度计数和最近活动。用户可以关闭电脑或切换对话；持久 Run 继续执行，并在有意义的变化、失败或完成时通知。简单查询仍保持同步快速路径，不应一律进入后台。

### 3.7 第七章：评测、失败归因与可观测性

[第七章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter7.md)强调先定义“成功”，再决定任务、验证器和统计方法。`Pass@k` 反映偶尔成功的能力上限，连续成功率更接近生产可靠性。评测不能只看最终答案，还要定位轨迹中的第一个错误，并区分：事情没做对、做对但说错、格式不匹配、作用域理解错误和精确参数错误。

生产评测需要两类任务：

- 端到端回归：完整用户问题是否得到正确回答；
- trajectory-prefix 边界回归：给定已经发生到某一步的轨迹，Agent 下一步是否正确使用当前信息、遵从最新指令、处理歧义和避免危险动作。

对当前 Debug 轨迹的映射：

- Debug 1–10 不应只作为人工阅读材料，而应转成版本化评测样本；
- 每个样本应有用户原话、期望 `ScientificIntent`、实体解析、候选能力、计划、允许/禁止工具、期望结果状态和回答 Rubric；
- 必须区分科学语义错误与格式/措辞错误；
- 对 GLM-5.3、其他模型和不同推理强度，应在同一任务集上比较准确性、工具合法率、证据覆盖、首次输出时间、总延迟、调用次数和成本；
- “把工具调用压缩到 5–15 次”只是机制指标，不是目标。真正目标是结论正确、覆盖充分、可追溯、延迟可接受且无退化；
- 每次修复都必须同时跑目标边界集和旧能力保留集。

### 3.8 第八章：什么时候需要后训练

[第八章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter8.md)区分 Mid-training、SFT 与 RL：底座缺知识/能力时补底座；模型偶尔能做但协议和格式不稳定时用 SFT；只有策略可探索、轨迹可评分、奖励有差异时才适合 RL。

这对当前项目的判断非常明确：现在不应优先微调 GLM-5.3。因为同一句用户请求尚未被 Harness 提供完整、稳定、可验证的语义—能力桥；训练模型只会把当前接口缺陷固化到参数中。应先做到：

1. Schema 稳定；
2. 工具结果可验证；
3. 失败可分类；
4. 评测集可重放；
5. 多模型在同一环境下仍表现出稳定剩余缺陷。

之后，如果问题主要是结构化输出不稳定，可考虑少量 SFT；如果基础知识缺失且无法外部检索，才考虑 Mid-training；如果有大量可验证轨迹和明确奖励，再考虑 RL。硬性统计规则永远不应依赖参数记忆。

### 3.9 第九章：从 Debug 轨迹到持续进化

[第九章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter9.md)将学习信号分为三层：结果验证器判断是否完成，过程验证器判断是否以允许方式完成，质量验证器判断交付质量。单一分数不足以学习；评价必须给出维度、证据位置、错误类型和置信度。

经验可以写入四个位置：知识、Prompt/Skill、程序或模型参数。选错位置会制造新问题：数据覆盖是知识问题，调用约束是程序问题，领域操作方法是 Skill 问题，稳定的语言策略才可能进入模型参数。

持续进化不能把每次用户抱怨直接追加进系统提示词。正确闭环是：

```text
在线记录轨迹
  -> 离线失败归因
  -> 生成最小更新提案
  -> 边界集 + 保留集 + 安全集验证
  -> 人工/策略审批
  -> 灰度发布
  -> 监控、回滚、定期整理
```

对本项目而言，Debug 轨迹应沉淀为 `eval case`，不是沉淀为越来越长的“禁止模型怎样做”提示。重复超时进入 Harness 的重试/熔断程序；癌种映射错误进入实体解析器和词表；模块漏用进入 capability registry/planner；统计误读进入 claim validator；表达问题才进入 Prompt 或 Skill。

### 3.10 第十章：多 Agent 的真正适用边界

[第十章](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter10.md)认为多 Agent 只有在引入新信息、独立验证、权限隔离或真实并行时才有价值。把相同模型复制成多个角色进行纯文本讨论，常常只是增加 token，并产生共享上下文膨胀、错误级联、同质趋同、责任推诿和循环失控。

对 DepMap 的建议：

- 简单查询与意图解析保持单 Agent；
- 数据证据与文献证据可以并行，但通过结构化移交包汇合；
- 创新性审核、统计审核、临床转化审核可作为上下文隔离的独立 Reviewer；
- Reviewer 只能读取 `EvidenceBundle`、候选课题和 Rubric，不应继承生成者的全部思考；
- Manager 必须有显式完成条件、取消机制、预算和依赖失败策略；
- 前置节点失败时，后续节点应显示具体依赖状态，并允许替换/重试该节点，而不是整条链静默阻塞。

### 3.11 后记：模型与 Harness 共同演进

[后记](https://github.com/bojieli/ai-agent-book/blob/main/book/afterword.md)再次回到 `Agent = LLM + 上下文 + 工具`，并提出实时交互和持续学习仍是两项未完全解决的问题。这提醒我们：当前架构既不能假设模型永远按轮次等待，也不能假设每次 Debug 后模型会自动记住教训。快路径查询、后台慢任务、事件通知和可打断执行需要在运行时实现；经验积累则需要版本化轨迹、验证过的更新提案和可回滚发布，而不是依赖同一对话的长期记忆。

---

## 4. 当前 DepMap Agent 已经具备什么

以下是工程能力，不是对服务器全部数据重新做的一次存储审计：

| 层 | 当前已有基础 | 判断 |
|---|---|---|
| 专用 Agent | 可选择 DepMap Specialist，项目可设置默认 Agent | 已有 |
| Skill | `depmap-knowledge-query`、`depmap-coding-agent` 及报告/文献相关 Skill | 已有，但选择机制仍需评测 |
| MCP/API | 本地/HTTPS 的有界只读查询，DepMap 与 TCGA 查询入口 | 已有 |
| 能力注释 | `knowledge-module-annotations.json` 描述模块、能力、实体角色、统计语义与声明边界 | 已有第一版 |
| 路由注册表 | `agent-capability-registry.json` 描述意图、槽位、执行等级与候选工具 | 已有第一版 |
| 多槽位意图 | 癌种、基因、表型、分子焦点、机制、证据源、输出、执行策略、未解析概念 | 已有雏形 |
| 实体解析 | 癌种规范化及 34 个 canonical lineage；基因/模块参数校验 | 部分已有 |
| 稀疏状态 | `FOUND`、`NOT_RETAINED`、`INELIGIBLE`、`NOT_COMPUTED`、`MODULE_UNAVAILABLE` | 已有 |
| 证据账本 | 成功查询保存稳定 `evidence_ref`，可恢复当前证据 | 已有 |
| 运行控制 | 新分析可升级为持久 Run/Workflow 并要求审批 | 已有基础 |
| 轨迹评测 | DepMap Agent eval suite 与 trajectory rubric | 已有基础 |
| UI 进度 | Agents 面板、DAG 状态、失败和重试 | 有基础，但长节点的细粒度进度不足 |

最重要的进展是：系统已经不再完全依赖 Prompt 猜测工具，开始把能力与声明边界放入版本化 JSON 注册表。这是正确方向，但仍只是桥梁的“桥墩”，不是完整桥面。

---

## 5. 我们尚未涉及或尚未完成的内容

### 5.1 知识和 Schema 层

1. **正式 JSON Schema 不完整**
   当前有 JSON 注册表和 Rust/Python 运行时校验，但缺少独立、版本化、可由 CI 全量验证的 JSON Schema 套件。

2. **结果行 Schema 未完全统一**
   不同 RDS/Parquet 模块的列名、实体角色、统计方向、FDR 范围、缺失状态和 Manifest 关系仍可能各自定义。

3. **全部存储模块与查询能力的双向审计不足**
   需要证明每个正式模块都被某个 capability 引用，同时每个 capability 的每个 query adapter 都指向实际存在且 QA 完成的模块。

4. **跨数据源概念本体不足**
   癌种、DepMap lineage、TCGA cohort、临床组织学不是同一概念；基因 symbol、Entrez、Ensembl；药物名称、化合物 ID、靶点；通路、TF regulon、表型均需独立实体类型和映射证据。

5. **表型概念与可计算代理缺少明确关系**
   “肿瘤细胞干性”可能指外部 stemness score、特定 marker set、功能实验或文献概念。当前标成 `NOT_COMPUTED` 是诚实的，但还需要表达可用代理、所需输入和不能声称的结论。

6. **版本与兼容策略不足**
   数据发布版、模块 Schema 版、能力注册表版、MCP 协议版和 Agent Prompt/Skill 版应能够组合追踪。

### 5.2 语义桥和规划层

1. **缺少稳定的 `ScientificIntent` 中间表示**
   多槽位已经出现，但仍需成为独立 Schema，而不是散落在工具参数和 Prompt 中。

2. **缺少模型提议 + 确定性验证的通用解析协议**
   不能只靠硬编码 alias，也不能无条件相信模型。模型应输出候选、置信度和歧义；Resolver 再用词表、知识图和数据覆盖验证。

3. **缺少能力图驱动的 Evidence Planner**
   当前 route 多数选择一个推荐查询；复杂课题需要基于槽位交集生成多模块计划、依赖、预算和停止条件。

4. **缺少集合语义**
   “转录因子”不是一个焦点字符串，而是一个实体集合；它需要被展开到兼容的 source/target/regulator/feature 角色，并控制查询规模。

5. **缺少跨分块联动抽象**
   分块只是存储实现。用户问题应面向逻辑数据集查询，Adapter 在服务端完成分区裁剪、分页、合并、去重和排序；模型不应知道 301 个文件并逐个调用。

6. **缺少从问题到可声明结论的编译检查**
   每个计划步骤都需要说明它支持什么 claim、不支持什么 claim，以及多个相关结果能否组合成同一个机制假设。

### 5.3 执行、聚合和验证层

1. **EvidenceBundle 聚合协议仍不够统一**；
2. **分页、游标、总命中数、截断原因和跨模块覆盖率需要统一**；
3. **统计方向、效应大小、FDR family、样本量和资格原因需要机器校验**；
4. **跨模块冲突和重复证据缺少统一消解规则**；
5. **科学结论验证器不足**，例如相关不能表述为因果、观察性合成致死不能表述为验证完成；
6. **结果为零时的诊断不足**，应回答是概念未映射、实体不存在、癌种不合格、未计算、未保留、筛选过严还是模块不可用；
7. **查询性能指标不足**，需要记录规划耗时、首个证据耗时、每模块耗时、载荷大小和缓存命中。

### 5.4 交互和异步运行层

1. 长任务阶段事件不够细，导致“运行中但 0 tokens/0 tools”；
2. 文献检索无稳定的活动心跳和中间产物预览；
3. 同步模型调用与持久 Workflow/Run 的升级边界还需更清晰；
4. 失败重试仍容易暴露“Token 上限”而没有展示真正的墙钟、外部服务和步骤预算；
5. 依赖节点失败后的替换、降级、部分交付和继续执行策略不足。

### 5.5 评测和持续进化层

1. Debug 1–10 尚未全部结构化为可重放测试；
2. 缺少覆盖口语、错别字、隐含意图、歧义癌种和跨概念组合的语义边界集；
3. 缺少模块覆盖召回率：预期应调用 N 个能力，实际调用多少；
4. 缺少科学声明精确率：每个结论是否能回指数据字段或文献；
5. 缺少跨模型稳定性和 `Pass consecutive@k`；
6. 缺少线上轨迹到知识/Skill/程序/参数的自动失败归因；
7. 缺少版本化灰度、回滚和长期规则清理。

---

## 6. “用户意图—数据 MCP 桥接不足”的根因

根因不是单一的“癌种映射错误”，也不是简单的“模型语义理解差”。它是八层断裂叠加：

### 6.1 表示断裂

过去把多维科研请求压成一个 `focus`。疾病、表型、分子类别、机制、证据源、分析策略和交付目标混在一条字符串里。模型即使理解了，也没有位置保存这种理解，后续自然会丢槽位。

### 6.2 本体断裂

自然语言概念没有全部连接到规范实体和实体集合。“转录因子”可能被映射成 `DOROTHEA_TF_ABC` 富集表，却没有映射成可在表达、依赖、CNV、突变、药敏、亚型和 TCGA 中复用的 TF 基因集合。

### 6.3 能力可见性断裂

数据目录描述“存了什么”，API 文档描述“怎么调用”，Skill 描述“如何做任务”，但模型需要的是“哪个能力可以回答哪个问题、要求哪些实体、返回什么统计、允许什么结论”。过去这层没有统一注册表。

### 6.4 规划断裂

一条复杂意图常需多个模块联合回答。若路由器只能选择一个工具，模型只能拿到一个局部切面；拿到 DoRothEA 富集不代表已经检索了所有 TF 相关 DepMap 证据。

### 6.5 执行断裂

底层以文件/分块组织，工具以单基因或单模式查询，用户却可能询问一个癌种中的一个基因集合或课题空间。缺少服务端集合查询、分区裁剪和聚合时，模型被迫做大量循环，继而撞上调用/时间预算。

### 6.6 结果语义断裂

`limit=20`、稀疏保留、样本不合格和模块缺失没有以统一字段呈现，导致展示限制被当作分析限制，空结果被当作生物学阴性。

### 6.7 声明验证断裂

模型拿到统计行后仍需知道能说什么。若没有 claim contract 和验证器，它可能把相关说成调控、把分组均值差说成相关、把 DepMap lineage 说成临床 HCC、把候选说成已验证靶点。

### 6.8 反馈闭环断裂

轨迹暴露的错误尚未统一转成首错标签、边界集和保留集。因此同类问题在换一个基因、癌种或模型后重新出现，团队只能继续人工 Debug。

可以把根因压缩成一句话：

> 我们过去主要建设了“数据层”和“对话层”，但缺少位于二者之间、可执行且可验证的领域控制平面。

---

## 7. 基于全书得到的更好方案

### 7.1 产品定位：垂直领域 Agent，而不是数据库聊天壳

DepMap Agent 应被定义为垂直科研 Agent：领域工具、统计政策和证据规则是核心；代码能力用于覆盖缺口；Workflow 用于持久复杂交付；MCP 是标准数据/工具边界；Skill 是可按需加载的方法包。它们不是互斥选项。

```text
Wisp Science Harness
├── Conversational DepMap Agent       自然语言入口与动态决策
├── Scientific Semantic Compiler      意图、实体、能力和计划桥接
├── Capability Registry               可发现的领域能力目录
├── Read-only DepMap/TCGA MCP          有界证据执行
├── Evidence Ledger + Validator       来源、统计语义、声明审查
├── Skills                            按需方法与交付规范
├── Run Manager                       新分析和长任务
└── Workflows                         固定的多阶段产物与独立审核
```

### 7.2 领域控制平面的七个耐久对象

### A. `ScientificIntent`

保存用户真正表达的多维意图，不把未知概念丢掉：

```json
{
  "schema_version": "wisp.scientific-intent.v1",
  "raw_request": "我做的是肝癌，给我 DepMap 能做的课题，主要聚焦肿瘤细胞干性，研究转录因子",
  "task_type": ["topic_discovery"],
  "disease_mentions": [{"text": "肝癌"}],
  "gene_mentions": [],
  "phenotypes": [{"text": "肿瘤细胞干性", "candidate_id": "tumor_cell_stemness"}],
  "molecular_focus": [{"text": "转录因子", "candidate_id": "transcription_factor"}],
  "mechanisms": [],
  "evidence_sources": ["depmap"],
  "requested_outputs": ["candidate_topics"],
  "execution_policy": "precomputed_only",
  "unresolved_concepts": []
}
```

### B. `EntityResolution`

把模型提出的候选与维护词表/本体核对，保留临床词和数据代理之间的区别：

```json
{
  "input": "肝癌",
  "entity_type": "disease",
  "candidates": [
    {
      "canonical_id": "depmap-lineage:Liver",
      "label": "Liver",
      "relation": "model_grouping_proxy",
      "confidence": 0.96,
      "requires_confirmation": false
    },
    {
      "canonical_id": "tcga:LIHC",
      "label": "Liver Hepatocellular Carcinoma",
      "relation": "patient_cohort",
      "confidence": 0.90,
      "requires_confirmation": false
    }
  ],
  "warnings": ["DepMap Liver is not identical to clinical HCC"]
}
```

### C. `CapabilityDefinition`

每个分析模块对 Agent 暴露统一能力，而不是只暴露目录名：

```json
{
  "capability_id": "lineage_expression_dependency",
  "questions": ["哪些表达特征与某癌种中的基因依赖相关"],
  "input_roles": ["lineage", "source_gene_or_set", "target_gene_or_set"],
  "entity_sets": ["all_expression_genes", "all_gene_effect_targets", "dorothea_tf_abc"],
  "scope": "within_lineage",
  "metric": "correlation",
  "multiple_testing_family": "source_by_targets_within_lineage",
  "eligibility": {"minimum_models": 10},
  "retention": {"sparse": true, "rule": "module manifest reference"},
  "query_adapter": "depmap_query:lineage_expression_dependency",
  "allowed_claims": ["association", "candidate regulator-dependency relationship"],
  "forbidden_claims": ["causal regulation", "clinical survival benefit"],
  "manifest_ref": ".../manifest.json"
}
```

### D. `EvidencePlan`

Planner 将意图编译成多个可解释步骤，而不是由模型自由试工具：

```json
{
  "plan_id": "...",
  "intent_ref": "...",
  "coverage": {
    "direct": ["TF expression-dependency", "TF co-dependency", "TF pathway enrichment"],
    "composable": ["TF CNV", "TF mutation", "TF PRISM", "TCGA expression-survival"],
    "not_computed": ["direct tumor-cell-stemness score association"]
  },
  "steps": [
    {"id": "resolve", "type": "entity_resolution"},
    {"id": "discover", "type": "capability_discovery", "depends_on": ["resolve"]},
    {"id": "query_tf_set", "type": "bounded_query", "fanout_policy": "server_side_set"},
    {"id": "aggregate", "type": "evidence_aggregation"},
    {"id": "validate", "type": "claim_validation"}
  ],
  "budgets": {"max_capabilities": 12, "max_rows_per_family": 50, "wall_seconds": 120},
  "stop_conditions": ["coverage_satisfied", "no_new_evidence", "budget_exhausted"]
}
```

### E. `QueryRun`

记录实际执行，而不是只保留最终文本：输入、能力、后端、版本、参数、分页、耗时、缓存、结果状态和错误分类。

### F. `EvidenceBundle`

统一聚合多个模块：

```json
{
  "bundle_id": "...",
  "intent_ref": "...",
  "analysis_scope": {"release": "26Q1", "lineage": "Liver"},
  "families": [],
  "coverage_summary": {
    "planned": 10,
    "queried": 9,
    "found": 7,
    "not_retained": 1,
    "ineligible": 0,
    "not_computed": 1,
    "module_unavailable": 0
  },
  "pagination": {"returned_rows": 20, "total_retained_rows": 384, "has_more": true},
  "evidence_refs": [],
  "coverage_gaps": []
}
```

### G. `Claim`

最终文本中的每个科学陈述都映射回证据：

```json
{
  "claim_id": "...",
  "text": "候选 TF X 的表达与基因 Y 依赖在 Liver 模型中相关",
  "claim_type": "association",
  "evidence_refs": ["depmap://..."],
  "scope": "DepMap Liver cell-line models",
  "strength": "hypothesis_generating",
  "validation": "PASS",
  "forbidden_upgrade": "causal transcriptional regulation"
}
```

### 7.3 模型和确定性程序如何分工

| 工作 | 模型 | 确定性 Harness |
|---|---:|---:|
| 理解口语、隐含研究目标 | 主责 | 保存原话 |
| 提出实体/概念候选 | 主责 | 校验候选是否合法 |
| 处理歧义并生成澄清问题 | 主责 | 判断何时必须澄清 |
| 判断模块是否存在/QA 完成 | 不负责 | 主责 |
| 检查输入角色和癌种资格 | 不负责 | 主责 |
| 构建候选能力集合 | 辅助排序 | 主责召回与过滤 |
| 规划多模块证据路径 | 提议 | 编译、预算和约束 |
| 执行查询、分页、分块联动 | 不负责 | 主责 |
| 计算统计量/FDR | 不负责 | 主责 |
| 综合证据、提出课题 | 主责 | 提供结构化证据 |
| 判断结论能否升级为因果 | 不单独负责 | Claim Validator 主责 |
| 生成自然语言报告 | 主责 | 检查引用、范围和完整性 |

这既不是纯硬编码，也不是把一切交给模型。硬编码的是不应变化的协议、类型和安全边界；数据驱动的是实体、能力、版本和统计元数据；模型驱动的是开放语义理解、候选生成、歧义处理与综合论证。

### 7.4 混合检索，而不是单一向量检索

建议采用四路召回：

1. 精确实体索引：基因 symbol、药物 ID、canonical lineage、TCGA cohort；
2. 词法检索：别名、缩写、中文临床名称；
3. 语义检索：口语化表型、研究目标、近义机制；
4. 能力图遍历：从已解析概念沿实体角色、输入兼容和输出 claim 找模块。

四路结果融合后，再由确定性兼容检查过滤，最后允许模型排序或解释。向量相似度不能直接决定执行工具，更不能直接作为科学证据。

### 7.5 工具的渐进式披露

```text
常驻上下文：
  resolve_scientific_intent
  discover_capabilities
  explain_coverage

发现后动态暴露：
  3–8 个与当前计划匹配的只读查询契约

覆盖缺口且获用户授权后：
  create_analysis_run
  monitor_run
  validate_run
```

这样既避免工具爆炸，也避免固定路由只看到一个模块。

### 7.6 查询层必须屏蔽物理分块

分块是为并行、断点、压缩和文件系统限制服务的物理实现，不是 Agent 语义。逻辑查询接口必须做到：

```text
logical query
  -> partition pruning
  -> parallel bounded reads
  -> schema normalization
  -> deduplication
  -> stable ordering
  -> cursor pagination
  -> aggregate metadata
```

模型只看逻辑数据集、命中总数、返回页和证据引用。不同分块当然要联动，但联动应在 MCP/API 服务端完成，不能让模型循环读取 301 个 Parquet 文件。

### 7.7 四级执行策略

| 等级 | 含义 | 典型动作 | 是否审批 |
|---|---|---|---|
| L0 | 语义澄清 | 解析口语、报告歧义 | 否 |
| L1 | 元数据/直接查询 | 状态、目录、单实体证据 | 否 |
| L2 | 多模块调查 | 计划、并行查询、证据聚合 | 否，只读有界 |
| L3 | 外部文献/独立评审 | 有边界检索、引用核验 | 视网络策略 |
| L4 | 新统计计算/持久交付 | Run、Workflow、写入产物 | 是 |

简单问题不强制 Workflow；多阶段可恢复交付才进入 Workflow。用户明确要求“只读取预计算”时，任何 `NOT_COMPUTED` 都只能作为覆盖缺口和后续建议，不能自动启动新计算。

---

## 8. 对“肝癌 + 肿瘤细胞干性 + 转录因子”的正确处理示例

### 8.1 不能再这样做

```text
识别到“转录因子”
-> 只查 DoRothEA enrichment
-> 返回前 20 行
-> 若没有显著行就说“没有结果”
```

这条路径同时犯了集合缩减、能力漏召回、展示上限误读和空结果误读四个错误。

### 8.2 正确路径

1. 保存原始疾病词“肝癌”，解析为 DepMap `Liver` 代理和 TCGA `LIHC` 患者队列，并标注二者不等价；
2. 将“肿瘤细胞干性”解析为表型概念，发现直接 stemness score 预计算缺失，保留为 `NOT_COMPUTED`；
3. 将“转录因子”解析为分子类别和实体集合，而不是单一富集表；
4. 从能力图召回所有兼容模块：表达—依赖、共依赖、表达相关、TF/通路富集、CNV—依赖、突变—依赖、PRISM、亚型、TCGA 表达—生存等；
5. 按 `direct / composable / requires_new_analysis / unsupported` 分类；
6. 在服务端对 TF 集合执行有界扫描和稳定分页，不由模型遍历物理块；
7. 聚合时保留每类分析的样本量、统计量、FDR、保留规则和覆盖状态；
8. 课题生成只能依据返回证据，明确哪些是直接结果、哪些是跨模块假设、哪些需要补算 stemness score；
9. 如果用户要“创新性”，再触发独立、可追溯的文献检索；
10. 每个课题由独立 Reviewer 检查证据覆盖、统计可行性、临床外推边界和已有文献冲突。

因此，即使直接“干性分数—TF—依赖”没有预计算，也不应只返回 `not found`。正确回答应同时给出：现有模块能直接支撑哪些方向、哪些方向可组合提出、直接干性分析缺什么、怎样补算，以及所有结论的边界。

---

## 9. 建议的实施路线

### Phase 0：冻结基线与建立双向审计

- 冻结数据版本、能力注册表版本和现有测试基线；
- 为所有正式模块生成 inventory；
- CI 检查“模块 -> capability”和“capability -> adapter -> manifest”双向可达；
- 标记存储存在但未暴露、接口存在但数据缺失、Manifest 非 complete 等状态。

验收：所有模块都有唯一 ID；任何未知/重复/悬空引用导致测试失败。

### Phase 1：正式 Schema 套件

建立独立 Schema：

- `scientific-intent.schema.json`
- `entity-resolution.schema.json`
- `capability-definition.schema.json`
- `evidence-plan.schema.json`
- `query-run.schema.json`
- `evidence-bundle.schema.json`
- `scientific-claim.schema.json`

验收：JSON、Rust DTO、Python API 和 MCP tool contract 使用同一版本；契约漂移在 CI 中失败。

当前进度（2026-09-05）：`ScientificEntityRegistry` 的第一阶段已经落地。
癌种与歧义集合、基因目录规则、PRISM 药物目录规则、通路候选规则和科学
概念命名空间已进入同一个版本化注册表；MCP 暴露统一
`depmap_resolve_entity`，Rust 路由在 `ScientificIntent` 中记录可检查的
`entity_resolutions`。基因/药物必须由安装目录核验；仅做拼写规范化的通路
保持 `NORMALIZED_UNVERIFIED`。后续仍需把通路全集建立成可直接核验的实体
目录，并将多实体解析改为一次批量调用。

### Phase 2：语义编译器与 Resolver

- 模型负责从自然语言产生候选 IR；
- Resolver 负责实体类型、规范 ID、歧义和代理关系；
- 未知概念保留在 `unresolved_concepts`，不静默丢弃；
- 用户不需要使用学术术语，也不需要说“依赖性”才触发 DepMap 能力发现。

验收：不同口语表述产生等价 IR；临床癌种与 DepMap lineage 不被混同。

### Phase 3：Capability Graph 与 Planner

- 由现有 annotations/registry 生成图；
- 支持实体集合、角色兼容、跨源组合；
- 输出 direct/composable/new-analysis/gap 四类覆盖；
- 对简单问题生成一个步骤，对复杂问题生成有界 DAG；
- Planner 不直接访问文件系统。

验收：肝癌+干性+TF 不能只召回 DoRothEA；计划覆盖率达到预设门槛。

### Phase 4：逻辑查询、分页与聚合

- 服务端屏蔽物理分块；
- 支持集合查询和批量投影；
- 统一游标、总命中、截断、状态和 provenance；
- EvidenceBundle 可写入账本并按引用回取。

验收：`limit=20` 永不被解释为总分析量；跨块结果稳定、无重复、可分页。

### Phase 5：统计和 Claim Validator

当前状态：**DepMap 最终回答的第一阶段硬门已实现**。DepMap Specialist 提交答案时必须
同时提交 Claim、当前轮次 `evidence_id` 和精确 JSON Pointer；宿主核对数值、显示精度、
稀疏状态及因果/临床越界后才允许发布，并自动附加机器核验证据索引。这不是 Prompt
约定。当前 TCGA 预计算结果和 QA 通过的新计算 Run 已进入同一数值 Pointer 核验；
声明了 `literature_search` 的检索交付也进入同一账本，但文献层目前只硬性验证
DOI/PMID/URL 等定位符与 Workflow/子工具来源，尚未自动判断“原文段落是否蕴含模型的
改写结论”。统一持久化 ClaimRecord 和 claim-to-passage 蕴含验证仍待实现。

- 为每个 metric family 建规则；
- 验证 effect direction、样本量、FDR family、癌种范围；
- 检查每个生成 Claim 的 evidence refs；
- 阻止相关到因果、细胞系到临床、候选到验证完成的语义升级。

验收：故意构造错误结论，验证器必须拒绝并指出具体冲突。

### Phase 6：异步状态与恢复

- 为规划、查询、聚合、验证、写作分别发进度事件；
- 增加首次输出时间、最近活动、步骤计数和 Watchdog；
- 重试依据错误类型而非统一重跑；
- 支持部分结果交付和失败节点替换。

验收：长文献任务不再十分钟显示 0 tools/0 progress；用户可取消且状态一致。

### Phase 7：评测驱动迭代

- 把 Debug 1–10 转成回归集；
- 扩展口语、歧义、隐含机制、未知概念和多槽位组合；
- 同时测首错、最终正确性、能力召回、非法调用、证据引用、延迟和成本；
- 每个修复用边界集和保留集；
- 比较 GLM-5.3 与其他模型的连续可靠性，不凭单次轨迹定优劣。

验收：同一版本的结果可重放；报告能指出首次失败发生在解析、发现、规划、执行、聚合、验证还是表达。

### Phase 8：可选的后训练

只有当 Harness 和评测稳定后，才根据剩余问题决定 SFT/RL。当前不把后训练作为桥接层缺失的替代品。

---

## 10. 必须建立的核心验收集

### 10.1 同义表达保持集

以下表达应解析到相同核心槽位，但保留措辞差异：

- “我做肝癌，想看肿瘤细胞干性和转录因子能做什么课题”；
- “Liver 里哪些调控蛋白可能维持 CSC 样状态？”；
- “不用先给基因，帮我从肝来源细胞系里找自我更新相关的调控方向”；
- “肝肿瘤的干性方向，优先看 TF，可不可以用 DepMap 做？”；
- “我不懂依赖性，你直接告诉我哪些转录调控课题有数据基础”。

预期：均保留疾病、表型、分子焦点和课题输出，不要求用户说出“CRISPR dependency”。

### 10.2 边界集

- “肝癌”不得静默等同于临床 HCC 的全部患者；
- “干性”不得自动声称已有 stemness score；
- “转录因子”不得只等于 DoRothEA enrichment；
- `NOT_RETAINED` 不得回答成“无关联”；
- `limit=20` 不得回答成“只分析了 20 个基因”；
- 相关性不得升级为转录调控因果；
- 未指定基因不得虚构 anchor gene；
- 未获授权不得启动新统计计算。

### 10.3 保留集

- 指定基因、不指定癌种；
- 不指定基因、指定癌种；
- 指定基因、指定癌种；
- 基因对、药物—基因、亚型、共扩增、True Love、观察性合成致死、3D、TCGA 表达—生存；
- 英文、中文、缩写、大小写和常见别名；
- 本地 MCP 与 HTTPS provider 返回一致的语义契约。

### 10.4 质量指标

| 指标 | 含义 |
|---|---|
| Intent slot recall | 用户表达的维度有多少被保留 |
| Entity resolution accuracy | 规范实体和代理关系是否正确 |
| Capability recall | 应用能力有多少进入计划 |
| Tool validity | 调用模式、参数和顺序是否合法 |
| Evidence coverage | 计划能力中成功查询/解释的比例 |
| Claim grounding precision | 结论能否回指真实证据 |
| Boundary violation rate | 因果、临床外推、阴性误读等违规率 |
| Time to first evidence | 首条可用证据出现时间 |
| End-to-end latency | 完整回答时间 |
| Pass consecutive@k | 连续多次可靠完成率 |

---

## 11. 明确不应采取的方案

1. 不把 180 GiB 结果转换成一个巨大 JSON 交给模型；
2. 不为每种用户说法手写一个 `if keyword then tool`；
3. 不把所有模块做成同时常驻的几十个工具；
4. 不把 Skill 当作数据证据或唯一调度器；
5. 不把 MCP 当作自动具备语义理解的 Agent；
6. 不让模型逐个遍历物理 Parquet/RDS 分块；
7. 不用固定 5–15 次调用作为“最好”的证明；
8. 不因一次 GLM-5.3 成功或失败决定架构；
9. 不把所有复杂任务强制塞进同一个 Workflow；
10. 不在桥接层未稳定时用微调掩盖接口问题；
11. 不把 Debug 修复持续追加到全局 Prompt；
12. 不允许模型自行宣称查询完成、QA 通过或数据不存在。

---

## 12. 对用户四个问题的直接回答

### 12.1 我们没有涉及哪些内容，还需要涉及哪些？

已经涉及 Agent、Skill、MCP、Workflow、部分 capability registry、多槽位意图、实体解析、Evidence Ledger、跨 DepMap/TCGA/已验证 Run 的数值 Grounding Gate、文献来源定位门、Run 和轨迹评测。尚需补齐的是正式 Schema 套件、完整领域本体、能力图、集合语义、Evidence Planner、服务端跨块聚合、统一 EvidenceBundle、文献 claim-to-passage 蕴含验证与持久化 ClaimRecord、细粒度异步状态、双向模块审计，以及从 Debug 轨迹到版本化持续进化的闭环。

### 12.2 我们的空白点和缺点是什么？

最大的空白不是“少一个工具”，而是缺少统一领域控制平面。现有部件之间仍有重复和割裂：Prompt/Skill 解释任务，JSON 注册表描述部分能力，Rust 路由限制调用，Python/MCP 查询数据，Workflow 编排长任务，但它们尚未完全共享同一套类型、状态、版本和验收标准。

### 12.3 “用户意图与数据 MCP 中间桥接不足”是什么原因？

核心原因是过去没有一个正式的语义编译过程，将自然语言的多槽位科研意图转换为经过实体校验、能力发现、兼容检查和预算约束的 `EvidencePlan`。癌种/基因映射只是其中一层；更深的问题是表型、机制、分子类别、实体集合、模块能力、统计语义和允许结论没有形成同一张可执行图。

### 12.4 书中能否给出更好的解决方法？

能。综合全书，最合适的方案是：

> 建设一个垂直领域 Agent Harness，以多槽位 `ScientificIntent` 为入口，以结构化 Capability Graph 为观察空间，以只读 MCP 为动作边界，以 Evidence Planner 编译多模块查询，以 Evidence Ledger 和 Claim Validator 保证可追溯与科学边界，以 Skills 做渐进式方法披露，以 Run/Workflow 承接持久复杂任务，再用轨迹评测和边界集持续进化。

这比“增加提示词”“预生成所有证据卡”“让模型读 180G”“为每个问题写固定 Workflow”都更稳健。它允许用户用不学术、甚至不完整的自然语言提出问题，同时确保最终调用来自已登记、已验证的分析模块，而不是模型常识或幻觉。

---

## 13. 最终架构判断

当前方向没有本质错误：预计算数据、MCP、Agent、Skill 和 Workflow 都是需要的。问题在于建设顺序曾更偏向“两端”——一端积累大规模分析结果，另一端做对话和工作流；中间的语义、能力和证据控制平面建设较晚。

现在应把开发重心从“继续增加模块/提示词/工作流”转为：

```text
先统一 Schema
-> 再完善实体与能力图
-> 再实现 Evidence Planner
-> 再统一查询聚合与 Claim 验证
-> 最后用 Debug 轨迹持续评测
```

完成后，180 GiB 数据不需要被模型读取，也不需要预先复制成上万个静态证据卡。Agent 会先理解用户问题，再从能力目录中发现相关分析，生成有界计划，通过 MCP 精确查询，聚合为小型证据包，并只输出能够被数据和文献追溯支持的课题与结论。这才是这些预计算结果真正转化为 Wisp Science Agent 能力的方式。

## 参考章节

- [全书目录](https://github.com/bojieli/ai-agent-book/tree/main/book)
- [第一章：AI Agent 入门](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter1.md)
- [第二章：上下文工程](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter2.md)
- [第三章：用户记忆与知识库](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter3.md)
- [第四章：工具](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter4.md)
- [第五章：Coding Agent 与通用 Agent](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter5.md)
- [第六章：交互、异步与观察/动作空间扩展](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter6.md)
- [第七章：Agent 评估](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter7.md)
- [第八章：模型后训练](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter8.md)
- [第九章：Agent 持续进化](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter9.md)
- [第十章：多 Agent 协作](https://github.com/bojieli/ai-agent-book/blob/main/book/chapter10.md)
- [后记](https://github.com/bojieli/ai-agent-book/blob/main/book/afterword.md)
