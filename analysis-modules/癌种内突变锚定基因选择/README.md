# 癌种内突变锚定基因选择

基于 DepMap 26Q1，为每个癌种列出可用于后续突变—依赖分析的锚定基因。正式队列取默认 Damaging、默认 Hotspot、Model 注释和 CRISPR Gene Dependency 四者的 ModelID 交集，保证菜单中的样本确实能够进入后续依赖分析。

- `Damaging`：依据 LikelyLoF，适合 TSG/功能丧失问题。
- `Hotspot`：依据官方热点规则，适合 OG/功能获得问题。
- 标准可分析门槛：Mut≥3、WT≥5。
- 严格推荐门槛：Mut≥10、WT≥5。
- 0 是该矩阵中未检出相应类型；缺失不当作阴性。

服务器正式结果位于模块的 `results_20260909_v3/`。`candidate_anchor_genes_by_lineage.csv` 是标准门槛菜单，`selectable_anchor_genes_by_lineage.csv` 是严格推荐菜单，`selectable_gene_counts_by_lineage.csv` 汇总每个癌种的严格候选数量。

脚本：`scripts/run_anchor_gene_selection_26Q1.R`。TSG 使用 Damaging，OG 使用 Hotspot；未分类基因保留在 Damaging 菜单中，但不自动赋予驱动角色。
