# 癌种内体细胞事件—基因依赖性分析说明

日期：2026-09-05  
结果版本：DepMap 26Q1  
直接构建脚本：`15_build_lineage_somatic_event_dependency.R`  
结果目录：`analysis-modules/癌种内突变锚定基因选择/cancer_anchor_catalog_v2/downstream_dependency/05_precomputed_gene_effect_matrices/lineage_custom_missense_mutation_dependency`

2026-09-09 起，本文涉及的六套全量突变依赖矩阵及候选汇总已从
`depmap-26q1-full/` 移入上述分析模块子目录。新位置保存真实文件，旧位置
不保留符号链接；查询端通过模块解析表读取新位置。

## 这次分析的意义

这次分析把 TM00 中少数基因的示例性分析扩展成一个可检索的、癌种分层的全基因组统计库。它系统回答下面的问题：

> 在同一种癌症中，携带基因 A 体细胞事件的模型，是否比不携带该事件的模型更依赖基因 B？

其中基因 A 是事件源基因，基因 B 是 CRISPR 依赖靶基因。结果可以从两个方向使用：

1. 给定一个突变基因和癌种，寻找可能的条件性依赖靶点；
2. 给定一个候选靶点，寻找可能预测该靶点依赖性的突变背景。

这是一套观察性候选发现数据。它适合提出合成致死或条件性脆弱性假设，但不能单独证明因果关系、药物敏感性或临床获益。

## 两个查询方向与当前知识库的对应关系

当前知识库直接分析的是 DepMap 癌细胞模型，不是患者临床样本。两个方向实际共享同一张“事件源基因 × CRISPR 靶基因”统计矩阵，只是查询入口不同。

| 查询方向 | 当前可执行的准确问题 | 主要输出 |
|---|---|---|
| 正向：事件找靶点 | 已知某癌种中基因 A 发生指定事件，查找哪些基因 B 被 CRISPR 敲除后在事件阳性模型中表现出更强依赖 | 固定 `lineage + source_gene`，按实际样本量、负向 `mean_difference` 和 FDR 筛选 `target_gene` |
| 反向：靶点找事件 | 已知一个 CRISPR 候选靶点 B，查找同一癌种中哪些基因 A 的事件与该靶点的增强依赖相关 | 固定 `lineage + target_gene`，跨事件源基因检索并重新处理跨源基因的多重检验问题 |

正向筛选寻找的是“哪个靶基因被敲除后产生更强的条件依赖”，不是寻找“哪个其他基因的突变”。反向筛选寻找的是细胞模型中的候选事件标志物；只有在患者队列完成事件频率、分型、预后或疗效验证后，才能讨论临床分型或联合用药。

在约 182.07 GiB 的 `depmap-26q1-full` 主体中，与这两个方向相关的模块如下：

| 模块 | 大小 | 事件口径 | 癌种层级 | 两个方向的实现程度 |
|---|---:|---|---|---|
| `lineage_custom_missense_mutation_dependency` | 70.135 GiB | 九类自定义编码区事件，包含 missense | 癌种内 | 两个方向的主要全量底库 |
| `lineage_damaging_mutation_dependency` | 6.228 GiB | DepMap 官方 damaging 矩阵 | 癌种内 | 两个方向的官方 damaging 口径对照库 |
| `lineage_hotspot_mutation_dependency` | 0.256 GiB | `Hotspot == TRUE` | 癌种内 | 两个方向的热点事件对照库 |
| `custom_missense_mutation_dependency` | 24.405 GiB | 同九类自定义事件 | 泛癌 | 支持两个方向，但不提供癌种特异解释 |
| `damaging_mutation_dependency` | 7.397 GiB | DepMap 官方 damaging 矩阵 | 泛癌 | 支持两个方向，但不提供癌种特异解释 |
| `hotspot_mutation_dependency` | 0.083 GiB | `Hotspot == TRUE` | 泛癌 | 支持两个方向，但不提供癌种特异解释 |
| `observational_synthetic_lethal_candidates` | 0.006 GiB | 汇总上述突变结果及 CNV 结果 | 泛癌候选汇总 | 可快速双向检索，但每事件只保留 Top 100，不能替代全量矩阵 |

以上六个突变全量模块合计约 108.50 GiB。另有约 47.45 GiB 的泛癌和癌种内 CNV 扩增—依赖性模块，可以执行结构相同的“扩增事件 ↔ 靶点”查询，但它们分析的是拷贝数扩增，不是体细胞突变。

`coamplification_dependency` 分析两个基因共同扩增后的靶基因依赖，适合研究组合扩增背景；`subtype_dependency` 分析分子亚型与靶基因依赖。两者属于相邻能力，不等同于单个突变源基因的正向或反向筛选。

当前 182 GiB 知识库没有直接实现以下临床步骤：

- 没有按患者突变状态计算临床样本的靶基因依赖，因为患者数据没有 CRISPR Gene Effect；
- 没有证明候选靶点对应药物一定有效；
- 没有完成患者疗效、预后、分型或联合用药验证；
- 没有把所有事件拆成具体位点、驱动/乘客、激活/失活或等位基因状态。

## 与 TM00 原脚本的关系

本次结果不是由 TM00 原脚本直接批量运行得到，而是由知识库构建脚本完成的全局扩展。

| 层次 | 脚本 | 作用 |
|---|---|---|
| TM00 事件定义 | `08_mutData_updata_23Q2.R` | 从 26Q1 体细胞突变长表定义自定义事件集合，并演示 ARID1A—HMGCR 分析 |
| TM00 正向筛选 | `09_batch_from_mut_to_target_23Q2.R` | 固定突变基因，遍历依赖靶基因 |
| TM00 癌种分层 | `10_batch_from_mut_to_target_23Q2_add_celltype.R` | 按癌种计算突变组与野生型组的依赖性差异 |
| TM00 反向筛选 | `11_batch_from_gene_to_mut_23Q2_add_celltype.R` | 固定靶基因，遍历可能的突变标志物 |
| 本次直接执行 | `15_build_lineage_somatic_event_dependency.R` | 扩展到所有合格癌种、事件源基因和 18,531 个 CRISPR 靶基因，并加入 Welch 检验和 BH FDR |

因此，这次分析的完整方法来源应记录为“TM00 08–11 的全局统计扩展”。当前结果 manifest 只写了 `tm00_reference: "08"`，它记录了事件定义来源，但没有完整表达 09–11 提供的筛选方向和癌种分层思路。

### 原始脚本中的直接代码依据

本次分析的不同部分并非都来自同一个 TM00 代码段，需要分别追溯：

| 本次规则 | 原始脚本依据 | 与原始脚本的关系 |
|---|---|---|
| 只取 `VariantInfo` 第一个后果 | `08_mutData_updata_23Q2.R:66` | 直接沿用 |
| 九类自定义事件集合 | `08_mutData_updata_23Q2.R:101-114` | 直接沿用，包括新增的 missense |
| 按 `ModelID × HugoSymbol` 构造事件矩阵 | `08_mutData_updata_23Q2.R:125-142` | 原脚本保存事件计数；本次转换成是否存在事件的布尔状态 |
| ARID1A 事件组与 HMGCR 依赖示例 | `08_mutData_updata_23Q2.R:210-239` | 提供“源事件基因 → 依赖靶基因”的基本问题和 `Mut ≥ 3、WT ≥ 5` 门槛 |
| 固定事件源基因，遍历所有靶基因 | `09_batch_from_mut_to_target_23Q2.R:121-189` | 本次扩展到所有合格事件源基因 |
| 按 `OncotreeLineage` 做癌种内比较 | `10_batch_from_mut_to_target_23Q2_add_celltype.R:36-53` | 直接继承癌种分层和入口门槛 |
| 固定靶基因，反查事件源基因 | `11_batch_from_gene_to_mut_23Q2_add_celltype.R:70-109` | 提供结果的反向查询用途 |
| Welch t、单侧 p 值、BH FDR | 无对应 TM00 原始实现 | 由 `15_build_lineage_somatic_event_dependency.R` 新增 |
| 所有癌种 × 所有合格源基因 × 18,531 靶基因 | 无对应 TM00 原始完整循环 | 由 builder 15 全局化产生 |

需要注意，TM00-09、10、11 读取的是 `mutData_damaging.rds`，而本次 70.14 GiB 模块使用的是 TM00-08 定义的自定义含 missense 事件集合。因此，本次分析组合了 TM00-08 的事件口径和 TM00-09/10/11 的问题设计；不能描述成对任意一份原始脚本的逐行复现。

## 输入数据

分析使用三个 DepMap 26Q1 数据表：

- `OmicsSomaticMutations.csv`：提供模型、基因和变异后果；
- `CRISPRGeneEffect.csv`：提供每个模型对 18,531 个基因的连续 Gene Effect；
- `Model.csv`：提供模型对应的 `OncotreeLineage`。

只使用 `IsDefaultEntryForModel == "Yes"` 的突变记录，并要求 `ModelID` 和 `HugoSymbol` 有效。

## “custom_missense”的真实定义

目录名中的 `custom_missense` 容易造成误解。脚本实际保留 `VariantInfo` 第一个后果属于以下九类的记录：

- `frameshift_variant`
- `stop_gained`
- `start_lost`
- `stop_lost`
- `splice_acceptor_variant`
- `splice_donor_variant`
- `missense_variant`
- `inframe_deletion`
- `inframe_insertion`

同一模型的同一基因只要有至少一条合格记录，就标记为事件阳性。不同位点、变异数目、等位基因状态、VAF 和具体功能效应没有分别建模。

所以这套数据更准确的含义是“癌种内广义编码区体细胞事件—依赖性”，并非仅分析错义突变。

### `isDeleterious` 是否使用

70.14 GiB 的 `lineage_custom_missense_mutation_dependency` 没有直接或间接使用 `isDeleterious` 字段。DepMap 26Q1 的 `OmicsSomaticMutations.csv` 实际表头中不存在 `isDeleterious` 和 `CCLEDeleterious`。完整 26Q1 长表提供了更丰富的替代注释，包括 `VariantInfo`、`VariantType`、`VepImpact`、`VepLofTool`、`OncogeneHighImpact`、`TumorSuppressorHighImpact`、`TranscriptLikelyLof`、`LikelyLoF`、`HessDriver`、`AMPathogenicity` 和 `Hotspot`。

本地另有旧版 DEMETER2 配套文件 `data/DEMETER2_Data_v6/CCLE_mutation_data.csv`（约 135 MB），其中确实包含 `isDeleterious`、TCGA/COSMIC hotspot 和等位基因计数字段。当前 TM00 迁移脚本和约 182 GiB 知识库构建脚本均未读取该文件；跨平台模块只使用了 DEMETER2 的 RNAi 依赖矩阵和样本信息。因此，旧版文件已经下载，并不代表当前突变—依赖性结果使用了它。

TM00-08 提到 `isDeleterious` 是为了说明数据版本演进：20Q4v2 使用 `isDeleterious`，23Q2 使用 `CCLEDeleterious`，26Q1 改为 `VariantType + VariantInfo`。本次脚本沿用 TM00-08 的替代方案，按照 `VariantInfo` 首个后果是否属于九类自定义集合来定义事件，并特别纳入 missense，以避免遗漏 BRAF V600E、KRAS G12D 等事件。

当前 builder 为复现和扩展 TM00-08，只读取 `ModelID`、`IsDefaultEntryForModel`、`HugoSymbol`、`VariantInfo` 和 `Hotspot`，没有使用上述 VEP、LoF、driver、pathogenicity 或癌基因/抑癌基因高影响标记。这是当前结果的明确方法边界：完整原始数据已经下载，但本模块只利用了其中一部分注释。后续应基于完整字段建立彼此分开的 `any coding event`、`likely LoF`、`driver/high-impact`、`hotspot` 和官方 `damaging` 事件层，而不是继续把所有九类事件混成一个二元状态。

#### “未检出事件”模型如何得到

原始突变长表没有为每个模型和每个无事件基因写一条阴性记录。当前脚本先把清洗后突变表中出现过任意突变记录的 `ModelID` 集合视为 `sequenced`，再与 `CRISPRGeneEffect.csv` 的模型取交集。对一个指定癌种和事件源基因：

```text
分析模型全集 = 该癌种 ∩ sequenced ModelID ∩ CRISPR Gene Effect ModelID
事件阳性模型 = 分析模型全集中存在合格 ModelID × HugoSymbol × VariantInfo 记录的模型
未检出事件模型 = 分析模型全集 − 事件阳性模型
```

这里的 `sequenced` 只要求模型在突变长表的任意位置出现过至少一条记录，并不是目标基因逐位点callability证明。因此补集只能解释为 `event_not_detected`，不能严格解释为已确认野生型。

当前输出的 `sample_order.csv` 保存该癌种分析模型全集；`mutation_gene_order.csv` 和RDS矩阵保存阳性/阴性数量，但没有保存每个事件源基因对应的具体阳性和阴性 `ModelID` 列表。若要审计具体模型归组，必须从 `OmicsSomaticMutations.csv` 按相同规则重建事件矩阵。下一版应持久化源基因—模型事件状态及其定义，最好包含 `positive / negative / unknown` 和判断依据。

知识库中另有两套使用 `OmicsSomaticMutationsMatrixDamaging.csv` 的结果：

- `damaging_mutation_dependency`：泛癌官方 damaging 口径；
- `lineage_damaging_mutation_dependency`：癌种内官方 damaging 口径。

这两套结果使用 DepMap 已经预计算好的 damaging 0/1 矩阵，可视为使用官方 damaging 定义的下游产物，但当前脚本仍没有读取或重新计算 `isDeleterious`。因此不能把“官方 damaging 矩阵”直接写成“本项目使用了 `isDeleterious` 字段”，除非另有该矩阵生成流程的版本化定义证据。

## 为什么使用 `Mut ≥ 3、WT ≥ 5`

这个门槛继承自 TM00 的探索性癌种分层流程，主要目的是在保留稀有事件的同时排除完全无法比较的分组。

- `Mut ≥ 3`：排除单个或两个突变模型主导的结果，使突变组至少能够估计均值和组内方差；
- `WT ≥ 5`：给对照组保留稍大的基线，用于估计野生型均值和方差；
- 分别设置两个门槛：保证一个事件在某癌种中同时存在事件阳性组和可比较的事件阴性组。

它是最低计算资格线，不是经过功效分析、负对照或参数敏感性分析确定的可靠性阈值。`3 vs 5` 的 Welch 检验统计效力很弱，容易产生不稳定的效应量和 p 值，尤其在模型异质性较大的癌种中。

还有一个实现细节需要特别记录：事件源基因在进入分析时要求总体 `Mut ≥ 3、WT ≥ 5`，但某个 CRISPR 靶基因可能有缺失值。脚本对具体靶基因的有效性检查实际是 `mutation_n ≥ 2、wildtype_n ≥ 2`。因此少数靶点的真实比较样本数可能低于入口门槛。输出 block 保存了每一组比较实际使用的 `mutation_n` 和 `wildtype_n`，后续候选筛选必须使用这两个字段再次过滤。

建议把样本量分成以下审查层级，而不是把入口门槛直接当成高置信标准：

| 层级 | 建议门槛 | 用途 |
|---|---|---|
| 可计算探索层 | Mut ≥ 3、WT ≥ 5 | 保留稀有事件，只用于初筛 |
| 常规候选层 | 实际 mutation_n ≥ 5、wildtype_n ≥ 10 | 降低极小样本比较的不稳定性 |
| 较稳健层 | 实际 mutation_n ≥ 10、wildtype_n ≥ 10 | 用于重点候选排序和敏感性分析 |

这些建议层级仍需通过发现数量、效应量稳定性、重采样结果和已知正负对照进行校准，不能预先视为最终标准。

## 实际统计方法

对于每个合格的“癌种 × 事件源基因”，脚本比较事件阳性组和事件阴性组在全部 18,531 个 CRISPR 靶基因上的 Gene Effect。

核心效应量为：

```text
mean_difference = mutation_mean_effect - wildtype_mean_effect
```

DepMap Gene Effect 越负表示敲除该基因造成的生长损害越强。因此：

- `mean_difference < 0`：事件阳性组对该靶基因更依赖；
- `mean_difference > 0`：事件阳性组对该靶基因依赖较弱。

脚本计算 Welch t 统计量和双侧 p 值，同时以“事件阳性组更依赖”为方向计算单侧 p 值。`fdr_mutation_more_dependent` 是在同一个“癌种 × 事件源基因”的 18,531 个靶基因内部，对单侧 p 值进行 BH 校正得到的。

这个 FDR 不是所有癌种、所有事件源基因和所有靶基因合并后的全局 FDR。

## 结果规模与存储结构

当前结果覆盖：

- 24 个癌种；
- 49,952 个“癌种 × 合格事件源基因”组合；
- 每个组合检测 18,531 个 CRISPR 靶基因；
- 总计约 925,660,512 组统计比较；
- 3,137 个未压缩 RDS block；
- 逻辑文件大小约 70.14 GiB。

体积最大的癌种为 Bowel、Uterus、Lung、Lymphoid 和 Skin。数据体积主要由完整统计矩阵决定，不代表显著候选数量。

### 文件格式构成

70.14 GiB 共包含 3,214 个文件，按实际服务器文件元数据拆分如下：

| 文件类别 | 数量 | 逻辑大小 | 占总大小 | 用途 |
|---|---:|---:|---:|---|
| 未压缩 RDS 统计块 | 3,137 | 70.1333 GiB | 99.9975% | 保存全部事件源基因 × CRISPR 靶基因统计矩阵 |
| 癌种 manifest JSON | 24 | 0.000695 GiB | 0.0010% | 保存癌种、样本数、参数和 block 执行记录 |
| 事件源基因索引 CSV | 24 | 0.000802 GiB | 0.0011% | 保存每个癌种的源基因顺序及入口 Mut/WT 数量 |
| 样本索引 CSV | 24 | 0.000017 GiB | 0.00002% | 保存每个癌种的 ModelID 顺序 |
| 靶基因索引 CSV | 1 | 0.000221 GiB | 0.0003% | 保存 18,531 个 CRISPR 靶基因的列顺序 |
| 分片汇总 CSV | 4 | 786 bytes | 约 0% | 汇总各癌种的样本数、源基因数和 block 数 |

因此，文件体积几乎全部来自 RDS。CSV 和 JSON 自身很小，主要负责解释如何读取 RDS。

RDS 是 R 的二进制对象格式。本模块使用 `saveRDS(..., compress = FALSE)`，没有压缩，方便快速读取大型矩阵，但磁盘占用较大。它不是普通二维数据表，不能直接用 Excel 打开；应使用 R 的 `readRDS()` 读取，或者编写程序逐 block 转换为 Parquet/CSV。

标准 block 覆盖 16 个事件源基因和全部 18,531 个靶基因。最后一个 block 可以少于 16 行。每个 block 是一个 R list，除版本、癌种和源基因范围外，包含 10 张同维度矩阵：

- 两张整数矩阵：`mutation_n`、`wildtype_n`；
- 八张双精度浮点矩阵：两组均值、均值差、Welch t、自由度、双侧 p、单侧 p 和单侧 FDR。

一个标准 block 的每张矩阵维度为 `16 × 18,531`，对应 296,496 个“事件源基因 × 靶基因”组合。10 张矩阵一起保存每个组合的样本量、效应量和显著性，而不是只保存显著行。这正是数据达到 70.14 GiB 的主要原因。

每个癌种目录包含：

- `sample_order.csv`：进入该癌种分析的模型顺序；
- `mutation_gene_order.csv`：事件源基因顺序和入口分组数量；
- `blocks/*.rds`：每 16 个事件源基因对应的完整靶基因统计矩阵；
- `manifest.json`：癌种、样本量、合格事件基因数和 block 运行记录。

每个 RDS block 保存以下矩阵：

| 字段 | 含义 |
|---|---|
| `mutation_n` | 该靶基因实际可用的事件阳性模型数 |
| `wildtype_n` | 该靶基因实际可用的事件阴性模型数 |
| `mutation_mean_effect` | 事件阳性组平均 Gene Effect |
| `wildtype_mean_effect` | 事件阴性组平均 Gene Effect |
| `mean_difference` | 阳性组减阴性组的均值差 |
| `welch_t` | Welch t 统计量 |
| `welch_df` | Welch 自由度 |
| `p_two_sided` | 双侧 p 值 |
| `p_mutation_more_dependent` | 阳性组更依赖的单侧 p 值 |
| `fdr_mutation_more_dependent` | 每个事件源基因内部的 BH FDR |

## 这套数据可以支持什么结论

合适的表述是：

> 在指定癌种的 DepMap 细胞模型中，携带某基因事件的模型对某 CRISPR 靶基因表现出更强或更弱的观察性依赖关联。

它可以用于：

- 生成癌种特异的条件依赖候选；
- 为潜在合成致死关系提供初筛线索；
- 从候选靶点反查可能的突变生物标志物；
- 选择后续细胞、类器官或独立筛选验证对象。

它不能单独证明：

- 事件源基因直接造成了靶基因依赖；
- 两个基因构成已经验证的合成致死关系；
- 靶向该基因一定产生药物敏感性；
- 结果能够直接预测患者疗效或生存；
- 不同变异后果具有相同功能。

## 当前主要审查风险

1. `custom_missense` 混合了功能差异很大的九类变异后果；驱动和乘客事件没有分开。
2. `Mut ≥ 3、WT ≥ 5` 只适合作为探索入口，部分靶点因 Gene Effect 缺失会进一步降到 `2 vs 2`。
3. 模型只做癌种内分组，没有控制分子亚型、共突变、拷贝数、突变负荷、培养条件和批次等混杂因素。
4. 每个事件源基因独立做 BH；没有控制约 9.26 亿次检验构成的全局发现空间。
5. 完整结果保存了非显著检验；不能把数据体积或总行数理解为发现数量。
6. 当前 manifest 没有完整记录输入文件校验和、代码版本及 TM00 09–11 的方法来源。

## 正式候选筛选建议

正式输出候选集合时，至少同时报告：

- 癌种、事件源基因和 CRISPR 靶基因；
- 具体事件定义，而不是只写 `custom_missense`；
- 实际 `mutation_n` 和 `wildtype_n`；
- 两组 Gene Effect 均值和 `mean_difference`；
- 单侧 p 值及其 FDR family；
- 最小效应量阈值；
- 对样本量门槛的敏感性分析；
- 已知混杂因素和外部验证状态。

在冻结正式候选前，应优先完成 `3/5`、`5/10`、`10/10` 样本门槛比较，并检查候选数量、排名重合率、效应量稳定性和已知正负对照。
