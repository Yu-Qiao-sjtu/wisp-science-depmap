# 转录因子活性—CRISPR基因依赖相关性分析

本模块完成 TM00 `05_from_gene_to_dependency.R` 第四部分留下的分析：先用 DoRothEA A–C 调控网络和 decoupleR ULM 从26Q1表达矩阵推断每个模型的转录因子活性，再把每个TF活性与18,531个CRISPR Gene Effect靶基因做Pearson相关。

方向解释：负相关表示TF活性越高，靶基因Gene Effect越负，即依赖越强；正相关表示TF活性越高，依赖越弱。该关系是跨细胞系观察性关联，不能单独证明TF直接调控靶基因或构成药物敏感机制。

## 统计契约

- 表达默认条目：`IsDefaultEntryForMC == Yes`，重复ModelID保留首条，与现有表达矩阵口径一致；
- TF网络：`dorothea_hs`，置信度A、B、C；
- 活性方法：decoupleR ULM，`minsize=5`，使用调控方向`mor`；
- 联合队列：TF活性与Gene Effect的ModelID交集；
- 相关：Pearson，按基因对完整观测；
- 覆盖门槛：原始配对相关和P值保留；只有有效配对数≥800的靶点进入FDR和Top排名，避免极少数观测产生的极端相关主导候选；
- FDR：每个TF内部，对达到覆盖门槛的依赖靶点独立BH；
- 全量结果：RDS分块；检索结果：每TF正/负各Top100及严格子集。

## 文件

- `scripts/build_tf_activity_dependency.R`：全量构建和`--test`合成缩小运行；
- `scripts/query_tf_dependency.R`：查询TF—靶基因配对或TF的双向Top结果；
- `scripts/validate_tf_activity_dependency.R`：验证分块覆盖、FDR覆盖门槛、Top行数和严格结果；
- 远程MCP：`depmap_tf_dependency_evidence`，支持精确配对和双向Top查询；
- `module.intent.json`：Agent触发语和参数要求；
- `results/结果数据索引.md`：服务器运行结果和验收记录。
