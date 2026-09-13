# DepMap Specialist 验收用例：肝癌、干性、转录因子

日期：2026-09-05。

## 原始输入

> 研究课题 ：肝癌，给我depmap能做的课题，基因功能聚焦在肿瘤细胞干性，基因主要是研究转录因子

真实模型回放时直接使用这条自然语言输入，不额外向用户输入中追加工具名称、执行顺序、参考答案或审批触发词。

## 要检验的行为

这是一个尚未指定候选基因的研究方向发现任务。Agent 应结合可用资源自主发现、筛选和论证候选方向，而不是要求用户先提供一个基因才能开始。

允许从领域知识或文献提出转录因子作为待查假设；候选需要经过证据核验，不能被说成用户已经指定的基因或预计算结果已经支持的结论。可以直接查询、按需查阅文献或采用合适的 Workflow，不预先规定唯一合法路径。

最终交付应保留三个条件：肝癌研究范围、肿瘤细胞干性、以转录因子为主要研究对象。每个推荐方向至少说明：

- 候选转录因子及其身份依据；调控靶基因或富集项不自动等同于候选基因本身属于转录因子。
- 与干性的关系来自什么证据；若只是待验证假设，应明确标注。
- 当前 DepMap 结果提供了什么观察，以及对应 release、cohort、指标和来源。
- 已有查询能回答什么，哪些分析需要新增计算，哪些主张需要独立实验验证。
- 推荐理由、主要局限和下一项能区分假设的验证。

没有足够证据时可以交付部分方向和具体缺口，不为了凑课题数量编造支持。也不把“某个 top-K 列表没有返回结果”直接当成“不存在相关课题”。

## 验收标准

| 维度 | 检查方式 |
|---|---|
| 目标保持 | 最终课题确实围绕干性和转录因子，不退化成任意药物或通路榜单 |
| 范围准确 | 保留用户的“肝癌”表述；Liver 只是本次 DepMap 分组代理，不自动证明精确病理类型 |
| 自主推进 | 不要求用户先指定基因；不因存在模板就强制启动 Workflow |
| 证据追溯 | 数值能回溯到成功查询或可核验产物；TF 身份、干性关联分别有依据或被明确标为假设 |
| 缺口处理 | 区分模块缺失、阈值过滤、稀疏保留缺失和生物学阴性；不自行降低统计标准来凑结果 |
| 交付实用性 | 给出可以进一步验证的研究问题、当前依据和执行可行性，而不是只有方向名称 |
| 授权与成本 | 查询/文献探索与新增大规模计算区分清楚；记录确认次数、重复调用、token、耗时和是否启动 Run |

评审整条轨迹和最终答案；不采用只看关键词是否出现的判分，也不禁止正确否定句中的科学术语。不预先固定“正确基因名单”，避免模型只需复述答案。

## 已执行的真实工具预检

本次直接使用仓库的 `DepMapEvidenceService`、项目 `.wisp/depmap-agent.json` 中配置的本地知识库和实际预计算文件。没有使用伪造数值，没有启动新的科学分析，也没有调用模型。

执行：

```powershell
& 'D:\New-PHD\depmap_0823\runtime\depmap-mcp-venv\Scripts\python.exe' target/depmap-liver-stemness-probe.py
```

原始响应保存在 `target/depmap-liver-stemness-tool-probe.json`。该文件记录原始任务、癌种解析、模块目录和完整的有界方向响应。

结果：

1. `肝癌` 解析为 `Liver`，状态为 `RESOLVED`，无须额外确认；响应同时标注这是模型分组代理。
2. 目录显示 9 个条目均可用，包括三个网络模块、CNV、三类 PRISM、通路/TF 富集和 TCGA 桥接。目录可用不等于每个问题都满足统计检验条件。
3. 三个网络模块 manifest 的 `lineage_sample_n` 分别为：effect correlation 25、expression correlation 28、expression-dependency 25。方向发现工具却对网络一律使用 `pair_n >= 30`，本次三类结果均返回 `NOT_RETAINED`、候选数 0。因此这些空结果不能直接被当成模块不存在或肝癌没有相关关系。
4. 通路/TF 富集模块的 manifest 记录 `tf_count=270`。但方向发现实际返回的 20 条富集候选全部为 `collection=PATHWAY`。混合榜单并未保证覆盖 TF 项，也没有验证候选 source gene 是否属于转录因子。
5. 方向工具只接受 `lineage` 和 `limit`；本次返回 20 个通用候选，没有“干性”或“转录因子类型”的筛选参数。它们不能直接被当作本任务的最终推荐。
6. 桌面原生 `depmap_query` 当前没有暴露 `lineage_directions` mode；本次预检调用的是 MCP 服务类对应的查询路径。接入本地 MCP 和只启用原生工具，是两个需要区分的实际测试环境。

源码定位：

- `services/depmap_api/app.py::_run_lineage_directions_query`：固定过滤条件与跨模块候选选择。
- `services/depmap_mcp/server.py::depmap_lineage_direction_discovery`：当前参数只包含 lineage/limit。
- `src-tauri/src/depmap_agent.rs::depmap_query_schema`：桌面原生 mode 列表。

## 按模块门槛修复后的同数据复测

随后已修复方向发现中的统一 30 样本门槛，改为读取各网络 manifest 的
`min_n`，并保留 FDR <= 0.05。复测仍使用相同本地文件和 `limit=20`，未重算
任何科学统计。原始预检文件保留，修复后响应另存为
`target/depmap-liver-stemness-tool-probe-after-threshold-fix.json`。

```powershell
& 'D:\New-PHD\depmap_0823\runtime\depmap-mcp-venv\Scripts\python.exe' target/depmap-liver-stemness-probe.py --output target/depmap-liver-stemness-tool-probe-after-threshold-fix.json
```

| 网络 | 实际采用的 min_n | 修复前返回候选数 | 修复后返回候选数 | 满足筛选的预计算行数 |
|---|---:|---:|---:|---:|
| effect_correlation | 10 | 0 | 20 | 11,295 |
| expression_correlation | 10 | 0 | 20 | 431,205 |
| expression_dependency | 10 | 0 | 20 | 34,199 |

返回的网络状态均为 `FOUND`，每一节同时记录门槛及其来源
`manifest.min_n`。这些是检索候选，不代表已经验证了干性机制或课题新颖性。
本次修复尚未增加 TF 身份或干性证据的筛选能力。

验证结果：API/MCP Python 测试 32 项通过，`wisp-mcp` smoke 通过，Rust
格式检查通过。通用工作区测试仍有两项失败：CLI 评测案例数断言（11/10）和
`project_policy_advertises_only_discovered_scientific_resources` 的
`depmap_read` 可用能力断言；后者独立复跑同样失败。完整日志为
`target/depmap-threshold-workspace-tests.log`，不能将本次工作区验证标为全绿。

## 当前判定及后续回放

**工具预检及样本门槛修复复测：已执行。Specialist 端到端验收：尚未执行，不能标记通过。**

本次终端未设置 live eval 所需的 Wisp 模型配置和凭据。真实模型回放应使用指定模型、新建 DepMap 会话、修订后的 Specialist，以及记录清楚的原生/MCP 工具配置。旧会话保留的角色提示词不能代表本次修复。

CLI 的通用 fixture eval 目前不等同于桌面 Specialist 装配，不能以 scripted 输出通过替代真实角色测试。

下一轮要观察模型能否识别剩余的候选覆盖限制，自主采用可用的进一步检索并交付符合目标的课题；不要把这个案例写成新的 Specialist 强制流程。样本门槛已经沿用模块自己的记录，更严格模块的门槛也会保留，未给所有数据另设统一低门槛。
