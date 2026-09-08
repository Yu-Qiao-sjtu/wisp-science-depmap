# 表达基因—CRISPR 基因依赖相关性分析

## 模块目的

本模块回答：某个基因的表达量变化，是否与细胞对另一个基因的 CRISPR 敲除敏感性相关。源变量是表达基因的 `log2(TPM + 1)`，靶变量是 CRISPR Gene Effect。它适合生成依赖性 biomarker 和机制候选，但相关性本身不能证明表达基因调控依赖靶基因，也不能直接证明药物疗效或合成致死。

负相关表示表达越高，Gene Effect 越低（更负），即细胞对靶基因敲除更敏感；正相关表示表达越高，Gene Effect 越高，即依赖更弱。

## 与 TM00 的关系

| TM00 脚本 | 原始问题 | 与当前矩阵的关系 |
|---|---|---|
| `04_predivtive_biomarkers.R` | 固定一个依赖靶基因（示例 ESR1），扫描哪些表达基因与其依赖相关 | 当前矩阵的一列 |
| `05_from_gene_to_dependency.R` | 固定一个表达基因（示例 ESR1、IL6R），扫描哪些基因的依赖与其表达相关 | 当前矩阵的一行 |
| `01_read_depmap.r` | 将表达量、Gene Effect 和 Model 对齐后保存 RDS | 解释 TM00 的共同样本与共同基因口径 |

26Q1 脚本 `05_build_expression_dependency_matrix.R` 将 TM00 的单基因扫描扩展为全部 `19,215` 个表达基因 × `18,531` 个依赖靶基因，共 `356,073,165` 个有方向的表达—依赖组合。

## 模块组成

| 目录/文件 | 内容 |
|---|---|
| `scripts/01_read_depmap.r` | TM00 输入清洗与三表对齐脚本 |
| `scripts/04_predivtive_biomarkers.R` | 固定依赖靶基因、扫描表达 biomarker 的 TM00 脚本 |
| `scripts/05_from_gene_to_dependency.R` | 固定表达基因、扫描依赖靶基因及下游富集的 TM00 脚本 |
| `scripts/05_build_expression_dependency_matrix.R` | 26Q1 全局完整矩阵生成脚本 |
| `scripts/17_build_lineage_sparse_networks.R` | 26Q1 癌种内稀疏网络脚本；其中 `family=expression_dependency` 属于本模块 |
| `scripts/18_verify_lineage_sparse_networks.R` | 癌种内结果数值复核脚本 |
| `scripts/22_finalize_lineage_networks_server.R` | 服务器覆盖率与结构验收脚本 |
| `scripts/query_expression_dependency_pair.R` | 从全局分块 RDS 查询一个“表达源—依赖靶”组合 |
| `scripts/脚本来源.md` | 原脚本位置和快照 SHA-256 |
| `data/输入数据清单.md` | 输入矩阵、对齐规则与校验值 |
| `results/结果数据索引.md` | 全局及癌种内结果的路径、规模和字段 |
| `results/ESR1查询测试记录.md` | 全局 RDS 的源/靶顺序与符号解释测试 |
| `分析方法与审查说明.md` | 计算公式、方向解释、覆盖范围与风险 |
| `module.intent.json` | agent 意图路由标签 |

## 完成状态

- 全局表达—依赖矩阵：已完成，1,140 个共同细胞系、19,215 个表达源、18,531 个依赖靶，约 4.050 GiB。
- 癌种内表达—依赖网络：已完成 24 个合格谱系，约 2.219 GiB。
- 两部分合计约 6.269 GiB，占 182.073 GiB 主知识库约 3.4%。

这里的“已完成”指相关性批量计算和结果落盘完成。TM00 04 中的 LASSO/随机森林，以及 TM00 05 中的 GSEA、PROGENy 和 TF 活性分析，不属于这两个矩阵结果。
