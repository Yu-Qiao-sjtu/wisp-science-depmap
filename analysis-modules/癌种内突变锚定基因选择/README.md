# 癌种内突变锚定基因选择

基于 DepMap 26Q1，为每个癌种列出可用于后续突变—依赖分析的锚定基因。正式队列取默认 Damaging、默认 Hotspot、Model 注释和 CRISPR Gene Dependency 四者的 ModelID 交集，保证菜单中的样本确实能够进入后续依赖分析。

- `Damaging`：依据 LikelyLoF，适合 TSG/功能丧失问题。
- `Hotspot`：依据官方热点规则，适合 OG/功能获得问题。
- `AnySelected`：`OmicsSomaticMutations.csv` 发布长表中该基因至少有一条保留记录，用于观察完整突变概况；它包含 Damaging、Hotspot 及其他入选变异，不直接作为功能性锚点推荐。
- Common Essential：使用同一 26Q1 发布版的 `CRISPRInferredCommonEssentials.csv`。完整概况保留这些基因并标记 `is_common_essential`。由于 KRAS、PIK3CA、VHL、BRCA2 等癌症相关锚点也在该表中，不应在突变锚点侧自动删除；排除版只用于敏感性分析。共同必需过滤更适合应用于后续依赖靶基因。
- 标准可分析门槛：Mut≥3、WT≥5。
- 严格推荐门槛：Mut≥10、WT≥5。
- 0 是该矩阵中未检出相应类型；缺失不当作阴性。

服务器正式结果位于模块的 `results_20260909_v6/`。`candidate_anchor_genes_by_lineage.csv` 是标准门槛菜单，`selectable_anchor_genes_by_lineage.csv` 是未过滤的严格菜单，`sensitivity_excluding_common_essential.csv` 仅用于评估排除共同必需基因的影响，`selectable_gene_counts_by_lineage.csv` 汇总每个癌种的严格候选数量。

脚本：`scripts/run_anchor_gene_selection_26Q1.R`。先用 AnySelected 查看癌种突变全貌；正式锚定时，TSG 使用 Damaging，OG 使用 Hotspot。三种突变来源分别与 Model 和 Gene Dependency 取交集，不要求不同突变矩阵彼此取交集。

变异类型、26Q1 实际数量、选择规则和用户意图映射见 `突变类型与选择指南.md`。`scripts/audit_mutation_types_26Q1.R` 从发布长表生成 `mutation_type_audit_v1/`，用于区分错义、移码、终止、剪接、框内 indel 等分子后果，并核对 LikelyLoF 与 Hotspot 的重叠；它只做概况审计，不替代正式的基因级 Mut/WT 矩阵。小型审计汇总同时保存在 `reports/mutation_type_overview_26Q1.csv`、`reports/damaging_hotspot_overlap_26Q1.csv` 和 `reports/mutation_type_audit_manifest_26Q1.json`。

`06-11突变脚本数理逻辑审计.md` 逐项还原六个原始脚本的队列、门槛、效应量、检验、FDR 和排序公式。结论是：06 用于锚点准入，07 的 Welch 检验与方向内 BH FDR 是正式统计核心，08 用于事件定义审计，09–11 的乘积 `Score` 只可作为探索性可视化优先级。对应的机器可读清单位于 `reports/mutation_script_math_audit_26Q1.csv`，26Q1 实际样本交集和缺失值核对位于 `reports/mutation_script_data_audit_26Q1.json`。

六个被审计的原始文件已归档到 `scripts/tm00-reference/`，并由 `source_manifest.json` 固定文件大小和 SHA256。它们用于历史追溯；新分析使用模块当前脚本和 `module.intent.json` 的统计契约。

## 知识卡片

`scripts/build_anchor_gene_cards.R` 将统计菜单与 HGNC、OncoKB 派生角色和 26Q1 Common Essential 标记合并。卡片包含癌种、基因、突变口径、Mut/WT 数量、频率、基因名称和类型、角色匹配、选择等级、解释与警示。`anchor_gene_cards_all.csv/jsonl` 用于完整检索，`anchor_gene_cards_strict.csv` 用于正式分析候选，`anchor_gene_cards_priority.csv` 是角色匹配且达到严格样本门槛的优先卡片。卡片用于选择和解释，不代表突变已被证明造成依赖或构成合成致死。

`scripts/build_all_mutation_gene_annotations_26Q1.R` 进一步生成一行一个基因的全量本地缓存。它覆盖依赖队列突变宇宙中的全部基因，合并 HGNC 身份、历史符号映射、癌种分布、Damaging/Hotspot/ProteinChange 可用性、主要变异类型、OncoKB 派生角色和 Common Essential 标记。全量表用于检索和生成知识卡片；OncoKB 空值表示本地角色快照未收录该基因，不表示抓取失败或该基因没有癌症意义。

全量缓存包含 19,784 行：19,775 个基因已解析到 HGNC，其中 19,739 个为直接符号匹配、36 个通过历史符号匹配；另外 9 个符号保留原始 DepMap 名称并标记为 `unresolved`。仓库保存 `reports/all_gene_annotation_cache_manifest_26Q1.json` 和 `reports/unresolved_hgnc_symbols_26Q1.csv` 供审查，约 9.7 MB 的 CSV 和 28.0 MB 的 JSONL 留在服务器私有模块中。

## 文档入口

- `突变信息分析模块总览.md`：模块首要入口；按目的、数据、事件定义、脚本流程、双向分析、当前结果、用户工作流和缺口完整梳理。
- `锚点基因筛选方法.md`：逐步记录模型交集、事件定义、癌种内计数、样本门槛、角色匹配、Common Essential 处理和输出等级。
- `问题梳理与结论.md`：汇总本模块建立过程中关于数据、分组、交集、样本门槛、Common Essential 和下游口径的疑问与结论。
- `突变类型与选择指南.md`：说明 26Q1 分子后果、事件定义及具体选择方法。
- `突变依赖双向分析与脚本索引.md`：盘点 TM00 的 01、06–11、14 号相关脚本，解释正向与反向查询、指标方向、完成度和现有缺口。
- `06-11突变脚本数理逻辑审计.md`：给出 06–11 的完整公式、统计边界、实际数据核对、风险和统一执行规范。
- `知识库提示.md`：供 Agent 检索和意图路由，定义触发语、决策规则、禁止混淆项和标准回答格式。
- `module.intent.json`：机器可读的模块角色和双向查询契约，明确 source 是突变事件、target 是 CRISPR 依赖靶基因。
