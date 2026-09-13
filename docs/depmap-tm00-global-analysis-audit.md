# DepMap 26Q1 全局分析与 TM00 原始脚本审计

日期：2026-08-31  
范围：`guotosky` 上 `depmap-26q1-full` 中 7 个 TM00 衍生全局模块  
状态：第一阶段方法与结构审计完成；P0 校准已完成；co-amplification lineage-adjusted 重算仍在运行

## 总结

现有 `validate_depmap_tm00_extensions.py` 的 `PASS` 只证明文件存在、manifest 状态、表结构和部分计数一致，不能证明全局化后的统计方法忠实于原脚本，也不能证明阈值已校准。

7 个模块中：

- 0 个可称为原脚本逐字等价的全局复现；
- 3 个属于方向合理但必须补校准的扩展；
- 3 个存在明确的统计解释或筛选范围风险；
- 1 个主要是下游汇总层，本身不重新计算 p 值，但受上游质量和 Top-N 截断影响。

最明确的问题是 canonical `bipolar_dependency_candidates` 仍允许 `pair_n >= 3`。审计已在真实数据中观察到仅 4–5 个共同模型、相关系数接近 ±1 的边，说明该模块会保留小样本伪极端相关。覆盖率网格重算确认 `pair_n >= 500` 与 `pair_n >= 800` 的结果完全一致，并与完整覆盖结果高度重合，因此后续正式候选应至少使用 500；旧 canonical 结果不得继续作为发布集合。

## 评级定义

- `忠实复现`：输入、样本、统计量、检验方向和输出语义与原脚本一致。
- `合理扩展`：保留原方法核心，并把固定示例参数化到全局；新增规则有清楚记录。
- `需校准`：方法方向合理，但关键样本量、作用量、FDR family 或稳定性阈值缺乏校准。
- `可能有误`：已发现会系统性产生不可靠候选或造成错误解释的实现/定义。

## 模块审计矩阵

| 模块 | TM00 来源 | 全局化改动 | 初步评级 | 主要问题 |
|---|---|---|---|---|
| `lineage_selective_dependency` | 脚本 14 的固定疾病背景对比 | 推广到 24 个 lineage，与所有其余 lineage 比较 | 需校准 | `min_lineage_n=10` 偏低；lineage-vs-rest 受 rest 组成和谱系不平衡影响；只要求 FDR，没有最小效应量；24 个 lineage 分别 BH，不是跨 lineage 全局 FDR |
| `progeny_prism_associations` | 脚本 05.3 的路径/药物示例 | 14 条 pathway × 1,482 个药物 | 可能有误 | `min_n=10` 可产生不稳定相关；逐 pathway BH 而非 20,748 次检验全局控制；未控制 lineage、批次和缺失机制；只凭 FDR 无最小 `|r|` |
| `observational_synthetic_lethal_candidates` | 脚本 05.2、07–13 | 汇总 4 个既有 mutation/CNV 模块 | 合理扩展但解释受限 | 本层不重算 p 值；每个 event 只取 Top 100 后再汇总，`evidence_family_count` 会受截断影响；不能称作验证过的 synthetic lethality |
| `coamplification_dependency` | 脚本 13 的 MYCN/DDX1 固定案例 | 20,988,320 方向性事件目录及 15,368 对高置信筛选 | 需校准 | CNV≥2、组 n≥25、amp n≤100、Jaccard≤0.4 均属项目阈值；BH 仅在每个方向性 pair 内跨 target，无法控制 284,784,408 次全局发现；共扩增组仍可能受 lineage/amplicon 混杂 |
| `bipolar_dependency_candidates` | 脚本 16、17 | 所有 source 的负相关 Top 20 | 可能有误 | 当前允许 `pair_n>=3`；真实数据已出现 n=4–5 的近完美伪相关；`r<=-0.3` 与 Top 20 未校准；逐 source BH；应在发布前重算 |
| `true_love_gene` | 脚本 17 的双极依赖示例 | 全基因严格互为负相关 rank-1，再做稳定性层 | 需校准 | `pair_n>=500` 是必要但经验性 QC；rank-1 对近似并列敏感；100 次 bootstrap 对 0.70 阈值不足；两 seed 共同通过 99 对，29 对跨阈值 |
| `lineage_selective_enrichment` | 脚本 03、05、13 的排序/富集思路 | 24 个 lineage 上 Hallmark+Reactome fgsea | 合理扩展但依赖上游 | fgsea 方法合理；结果继承 lineage-vs-rest 的混杂；每个 lineage 内 padj，不是跨 24 lineage 的全局控制；需检查基因 ID 去重和 rank ties |

## 已完成的数值交叉验证

### True Love Gene

- 原始 CSV 与 `effect_correlation/gene_order.csv` 均为 18,531 个基因，列顺序完全一致；
- ASB7/SUV39H1、CCNF/E2F1、TP53/MDM2 的独立 Pearson 重算与 block 结果完全一致；
- TM00 原脚本直接排序会纳入 n=4–5 的稀疏极端相关；`pair_n>=500` 能正确排除；
- 100-bootstrap：seed 2601 得到 110 对，seed 2602 得到 117 对，共同通过 99 对，Jaccard 0.7734；
- 500-bootstrap：seed 2601 得到 108 对，seed 2602 得到 115 对，共同通过 105 对，union 118，Jaccard 0.8898；
- 500-bootstrap 的跨 seed 不一致配对降至 13 对，说明增加重复次数显著降低了阈值噪声，但尚未完全消除边界不确定性；
- 500-bootstrap 输出的 manifest 元数据已修正为实际的 500 次重采样；计算中断产生的临时 checkpoint 已移入私有归档，不与完成状态混放。

详见 `docs/true-love-tm00-cross-validation.md`。

### Bipolar dependency 覆盖率敏感性

同一相关矩阵、`r <= -0.3`、逐 source BH FDR <= 0.05、每个 source Top 20 下，仅改变共同模型数：

| 最小 pair_n | 方向边 | 唯一配对 | reciprocal Top-K |
|---:|---:|---:|---:|
| 300 | 9,248 | 6,375 | 2,873 |
| 500 | 5,825 | 3,556 | 2,269 |
| 800 | 5,825 | 3,556 | 2,269 |
| 1,208（完整覆盖） | 5,241 | 3,187 | 2,054 |

- `n>=500` 与 `n>=800` 完全一致；
- `n>=500` 与完整覆盖的配对 Jaccard 为 0.8946；
- `n>=300` 与 `n>=500` 的 Jaccard 仅为 0.5556；
- canonical `n>=3` 有 115,502 个唯一配对，其中只有 3,336 个也出现在 `n>=500` 集合中。

结论：500 是当前数据上有经验支持的最低覆盖门槛；完整覆盖可作为高严格度子集。该结论只校准覆盖率，不解决逐 source FDR、Top-20 截断和生物学外部验证问题。

### PROGENy–PRISM 阈值敏感性

在原 20,748 个 pathway-drug 检验上，按每条 pathway 对满足覆盖门槛的药物重新计算 BH：

| 条件 | 显著关联数 |
|---|---:|
| 原规则：FDR <= 0.05 | 1,179 |
| FDR <= 0.05 且 `abs(r)>=0.2` | 185 |
| `n>=500` 且 FDR <= 0.05 | 462 |
| `n>=500`、FDR <= 0.05 且 `abs(r)>=0.2` | 30 |
| `n>=500`、FDR <= 0.05 且 `abs(r)>=0.3` | 6 |

`n>=10/30/50/100` 的结果完全相同，因为该区间没有额外剔除检验；到 `n>=300` 才减少 658 个测试。原流程的首要缺口因此是没有最小效应量，而不是 manifest 中写出的 `min_n=10` 本身。建议报告至少同时给出 `abs(r)>=0.2` 敏感性集合，且在 lineage-adjusted 复算前仍只作探索性关联解释。

### Co-amplification

现有结构验证已确认：

- MYCN/DDX1 smoke 分组为 14/13；
- 方向性 pair catalog 为 20,988,320；
- discovery 层为 15,368 对；
- 61 个 shard 的 pair 数、hit 数和 manifest 总数可对账。

这些检查尚不能回答阈值是否最优、lineage 混杂是否消除或跨 pair 假发现是否受控。正在运行的 lineage-adjusted 层是必要补充，但完成后仍需做与未调整结果的敏感性比较。

## 原脚本与全局扩展的共同差异

1. 原脚本多数是固定基因、固定癌种或示例性探索，并未定义全局发现空间。
2. 全局化显著扩大检验次数，因此原脚本中的 nominal p 或局部 FDR 不能自动升级为全局错误率控制。
3. 多数新增阈值是合理工程选择，但未通过负对照、参数网格或独立数据校准。
4. 原脚本常依赖 `cor.test`、Top-N 或绘图观察；直接全局化会放大缺失值、小样本和极端相关问题。
5. manifest 记录了方法，但部分模块没有记录输入 checksum、参数敏感性或与原示例的数值对照。

## 必须执行的修复与校准

### P0：发布前阻断

1. 重建 `bipolar_dependency_candidates`：把最小共同模型数参数化，至少比较 300/500/800/完整覆盖；旧结果标记为含低覆盖边，不得直接发布。
2. 将 `progeny_prism_associations` 的 `min_n=10` 提高并做 30/50/100 的敏感性分析，同时加入最小 `|r|`。
3. True Love 使用至少 500 次重采样并报告区间；把跨 seed 不一致配对标记为边界候选。
4. 所有报告必须把 `within-source`、`within-pathway`、`within-pair` FDR 与全局 FDR 区分。

### P1：混杂与外部验证

1. lineage 分析加入最小效应量，并比较 lineage-vs-rest 与加权/分层模型。
2. co-amplification 比较原 Welch、lineage-adjusted 模型，并评估 amplicon 重复事件。
3. PROGENy–PRISM 至少控制 lineage；必要时加入稳健或 Spearman 敏感性分析。
4. 负相关网络评估 common-essential、低方差、线粒体、染色体邻近和已知 CRISPR 偏差。
5. 用 CORUM/BioGRID、已知调控负相关对、Sanger/独立筛选或药物数据做外部验证。

### P2：审计可重复性

1. manifest 增加所有输入 SHA-256、代码版本、完整参数和随机种子；
2. 每个模块增加原 TM00 示例的独立数值重算，而不只是检查结果行存在；
3. validator 分成 `structural_qa` 与 `scientific_audit`，避免结构 PASS 被误读为方法已验证；
4. 保存参数网格的候选重叠率、效应量分布、发现数量和负对照结果。

## 当前可用性判断

- 可用于探索性查询：7 个模块均可，但必须保留各自 interpretation boundary。
- 可用于正式“发现集合”：目前没有模块应仅凭现有结构 PASS 直接冻结。
- 优先可修复：True Love 已有跨 seed 审计基础；co-amplification 正在生成 lineage-adjusted 结果。
- 优先需重算：bipolar dependency 和 PROGENy–PRISM。
