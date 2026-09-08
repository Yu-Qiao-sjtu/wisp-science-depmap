# 损伤性突变与基因依赖概率差异分析

本模块新增 26Q1 Dependency 概率口径分析，不覆盖既有 Gene Effect 结果。
服务器根目录：/home/data/gz0548/depmap-26q1/analysis-modules/损伤性突变-基因依赖概率差异分析

## 科学问题与输入

比较某基因损伤性突变阳性和阴性细胞系的 CRISPR 依赖概率。
输入为同版 OmicsSomaticMutationsMatrixDamaging.csv、CRISPRGeneDependency.csv、Model.csv。
Damaging 基于 LikelyLoF=True；1/2 为阳性、0 为阴性，NA 排除，阴性不等于没有任何其他变异。
不将 2 解释为已证明双等位完全失活。
官方依据：
https://storage.googleapis.com/shared-portal-files/Tools/26Q1_Mutation_Pipeline_Documentation.pdf

## 方法与原始 07 的关系

原始参考：D:/New-PHD/depmap/tm00-script/scripts/07_mutant_dependency_26Q1.R。
沿用双侧 Welch t 检验与 Dependency 连续概率，正的 Mut-control 差值表示突变组依赖概率更高。
每个基因对按有效依赖观测检查 Mut>=5、control>=10。
正向 BH 在单个突变基因的全部靶点内计算；反向 BH 在单个靶点的全部合格突变源内独立计算。
候选为观察性关联，不能直接认定合成致死；本轮为泛癌，癌种内复核尚未执行。

有意修复：不读取被 01 覆盖裁剪的 cellinfor.rds，而取三份原始数据的 ModelID 交集；
不将突变 NA 填成 0；拒绝重复默认模型和重复基因符号，避免静默选择。
因此不是复刻共享 RDS 导致的 1140 样本子集。旧本地结果不改动。
先以本次同一队列逐项对照原生 t.test 验证两个方向，不宣称与旧样本数值相同。

## 执行与验证

Rscript scripts/run_damaging_dependency_probability.R --test
Rscript scripts/run_damaging_dependency_probability.R /path/to/source /path/to/new-run

先运行合成数据测试，再读取输入并保存质量检查、样本可用性和基因分组计数。
全量前执行 ARID1A 对所有靶点及全部合格突变源对 HMGCR 的原生 t.test 对照，失败即停止。
随后分块保存统计量及正向 FDR，另存反向 FDR。
只在最终写出 manifest.json status=complete 后认定完成。
本版不自动断点恢复；失败时保留结果用于审查，以新运行目录重跑，避免混入旧块。

## 输出

- input_qc.json：实际样本、基因数量与缺失统计。
- sample_order.csv / sample_annotations.csv：分析队列及完整注释。
- sample_availability.csv：输入模型并集及三类覆盖标记，可追踪排除原因。
- all_mutation_gene_counts.csv：全部突变基因的阳性、阴性、未知数量和准入标记。
- mutation_gene_order.csv / target_gene_order.csv：结果索引。
- ARID1A_batch_target_screening.csv / HMGCR_batch_mutation_screening.csv：双方向示例。
- blocks/block_*.rds：组内有效样本数、均值、差值、双侧 P、正向 FDR。
- blocks/reverse_fdr_*.rds：独立反向 FDR，使用相同块顺序。
- manifest.json：完成状态、输入 MD5、方法、维度、测试结果。
- logs/：服务器日志。大数据不纳入 Git。
