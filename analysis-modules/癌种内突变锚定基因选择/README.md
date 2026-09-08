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

## 知识卡片

`scripts/build_anchor_gene_cards.R` 将统计菜单与 HGNC、OncoKB 派生角色和 26Q1 Common Essential 标记合并。卡片包含癌种、基因、突变口径、Mut/WT 数量、频率、基因名称和类型、角色匹配、选择等级、解释与警示。`anchor_gene_cards_all.csv/jsonl` 用于完整检索，`anchor_gene_cards_strict.csv` 用于正式分析候选，`anchor_gene_cards_priority.csv` 是角色匹配且达到严格样本门槛的优先卡片。卡片用于选择和解释，不代表突变已被证明造成依赖或构成合成致死。
