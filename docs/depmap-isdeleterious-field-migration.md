# DepMap `isDeleterious` 字段迁移与新版注释说明

日期：2026-09-05  
适用范围：DepMap 20Q4 至 26Q1 体细胞突变数据  
目的：说明旧版 `isDeleterious` 在新版数据中的对应信息，并指导当前突变—依赖性分析选择事件口径。

## 核心结论

旧版 `isDeleterious` 没有一对一改名为某个26Q1字段。DepMap先将它标记为过时并停止推荐，随后更新突变调用与注释管线，将原来一个粗粒度布尔值承担的用途拆解到变异后果、失活预测、癌症驱动证据、热点证据和致病性预测等多类注释中。

当前最接近旧字段用途的官方结果是：

```text
LikelyLoF
    ↓ 按模型和基因聚合
OmicsSomaticMutationsMatrixDamaging.csv
```

但 `LikelyLoF`、官方damaging矩阵和旧 `isDeleterious` 的算法与数据管线不同，不能视为数值等价或直接改名。

## 官方证据和解释边界

DepMap工作人员在2020年明确说明，`isDeleterious` 是过时注释，不建议继续使用，并计划在后续版本删除；当时建议使用 `Variant_annotation`。这是旧字段被废弃的直接官方依据：

- [DepMap论坛：How is the isDeleterious column determined?](https://forum.depmap.org/t/how-is-the-isdeleterious-column-in-the-ccle-mutations-csv-file-determined/129/2)

20Q4的 `CCLE_mutations.csv` 确实包含 `isDeleterious`。Bioconductor的DepMap 20Q4数据文档保存了该文件及字段结构：

- [Bioconductor DepMap 20Q4数据文档](https://s3.jcloud.sjtu.edu.cn/899a892efef34b1b944a19981040f55b-oss01/bioconductor/3.13/data/experiment/manuals/depmap/man/depmap.pdf)
- [DepMap 20Q4 Public归档](https://figshare.com/articles/dataset/DepMap_20Q4_Public/13237076)

26Q1官方突变管线文档没有再出现 `isDeleterious`，而是描述新版突变调用、过滤、rescue、注释字段及两个官方突变矩阵：

- [DepMap 26Q1 Mutation Pipeline Documentation](https://storage.googleapis.com/shared-portal-files/Tools/26Q1_Mutation_Pipeline_Documentation.pdf)
- [DepMap 26Q1当前发布数据页面](https://depmap.org/portal/data_page/?tab=currentRelease)

官方文档没有使用“把 `isDeleterious` 拆成以下字段”这一表述。因此，本文中的“拆分”是对数据模型演变的功能性解释。官方能够直接支持的事实是：旧字段被废弃、突变管线发生更新、新版提供多维注释、官方damaging矩阵由 `LikelyLoF` 生成。

## 版本演变

| 阶段 | 主要文件 | 主要字段或产物 | 状态 |
|---|---|---|---|
| 20Q4 | `CCLE_mutations.csv` | `isDeleterious`、`Variant_Classification`、TCGA/COSMIC hotspot等 | `isDeleterious`当时存在，但已被官方称为过时 |
| 过渡期 | 后续季度的CCLE突变表 | 官方一度建议使用 `Variant_annotation`；22Q4至23Q2前后突变调用和注释逻辑有较大更新 | 不能假设跨版本字段或damaging状态完全可比 |
| 26Q1 | `OmicsSomaticMutations.csv` | VEP、LoF、OncoKB、Hess、AlphaMissense、hotspot等多维注释 | 当前突变级明细表 |
| 26Q1 | `OmicsSomaticMutationsMatrixDamaging.csv` | 基于 `LikelyLoF == True` 聚合的模型×基因矩阵 | 当前官方damaging口径 |
| 26Q1 | `OmicsSomaticMutationsMatrixHotspot.csv` | 基于Hess、OncoKB、COSMIC及特定事件规则聚合 | 当前官方hotspot口径 |

## 从旧字段到新版信息的功能映射

下面不是字段改名表，而是“旧字段承担的问题，在新版中应到哪里寻找答案”的映射。

| 旧 `isDeleterious` 隐含的问题 | 26Q1字段或产物 | 新版含义 |
|---|---|---|
| 是否检测到一个变异 | 一条 `OmicsSomaticMutations.csv` 记录；`Chrom`、`Pos`、`Ref`、`Alt`、`ModelID` | 变异存在性由记录本身表达，不需要deleterious标记 |
| 变异是什么后果 | `VariantType`、`VariantInfo`、`MolecularConsequence` | SNV/indel及missense、frameshift、stop等序列后果 |
| VEP预测影响多大 | `VepImpact` | VEP影响等级，不等于癌症驱动性 |
| 是否可能造成功能缺失 | `LikelyLoF`、`TranscriptLikelyLof`、`VepLofTool` | 变异级或转录本级LoF证据 |
| 是否处于癌基因并具高影响 | `OncogeneHighImpact` | OncoKB癌基因背景且 `VepImpact == HIGH` |
| 是否处于抑癌基因并具高影响 | `TumorSuppressorHighImpact` | OncoKB抑癌基因背景且 `VepImpact == HIGH` |
| 是否属于已知驱动热点 | `HessDriver`、`HessSignature` | Hess等定义的驱动热点及其突变签名 |
| 是否属于癌症热点 | `Hotspot` | 综合OncoKB、COSMIC、Hess和若干特定事件规则 |
| missense是否可能致病 | `AMClass`、`AMPathogenicity`、`RevelScore`、`Sift`、`Polyphen`、`ProveanPrediction` | 多种蛋白影响与致病性预测，不能彼此替代 |
| 是否满足官方damaging口径 | `OmicsSomaticMutationsMatrixDamaging.csv` | `LikelyLoF == True`的基因—模型聚合结果 |
| 是否满足官方hotspot口径 | `OmicsSomaticMutationsMatrixHotspot.csv` | 官方热点规则的基因—模型聚合结果 |

## 26Q1官方damaging规则

26Q1文档明确说明：一个变异在 `LikelyLoF == True` 时被视为damaging，并进入 `OmicsSomaticMutationsMatrixDamaging.csv`。

`LikelyLoF` 的26Q1定义为：

```text
OncoKB mutation effect 为
“Likely Loss-of-function”或“Loss-of-function”
或者
VEP impact == HIGH
```

官方模型×基因矩阵取值为：

| 值 | 含义 |
|---:|---|
| 0 | 未检出符合该矩阵定义的事件 |
| 1 | 至少一个符合条件的事件，事件等位基因频率总和不高于0.95 |
| 2 | 至少一个符合条件的事件，事件等位基因频率总和高于0.95 |

因此，做当前版本的官方damaging分析时，应直接引用 `LikelyLoF` 和官方矩阵规则，不应把矩阵描述为旧 `isDeleterious` 的原样复制。

## 为什么从一个字段变成多维注释

旧布尔字段无法区分以下生物学问题：

- 一个变异是普通蛋白后果、可能失活，还是明确驱动事件；
- 同一个missense是激活型癌基因热点，还是可能无功能意义的乘客事件；
- 抑癌基因LoF和癌基因Gain-of-function是否应使用同一规则；
- 判断来自VEP、OncoKB、COSMIC、Hess、AlphaMissense还是功能实验；
- 不同证据发生冲突时，用户应如何追踪依据。

新版保留各类证据字段，并在特定用途下生成damaging或hotspot矩阵。这样可以按研究问题选择事件口径，也能追踪每个分类的证据来源。

上述解释符合26Q1管线结构，但“为什么拆分”的完整设计动机没有在官方文档中以单独章节明确陈述。正式报告中应写成“旧字段被废弃，新管线改用多维注释”，不应声称官方宣布了逐字段拆分方案。

## 与本地数据的对应关系

本地仍保留旧版DEMETER2配套文件：

```text
D:/New-PHD/depmap_0823/data/DEMETER2_Data_v6/CCLE_mutation_data.csv
```

该文件约135 MB，含 `isDeleterious`，属于旧版突变级注释输入。当前约182 GiB知识库没有使用它。

当前26Q1文件包括：

```text
D:/New-PHD/depmap_0823/data/OmicsSomaticMutations.csv
D:/New-PHD/depmap_0823/data/OmicsSomaticMutationsMatrixDamaging.csv
D:/New-PHD/depmap_0823/data/OmicsSomaticMutationsMatrixHotspot.csv
```

70.14 GiB的 `lineage_custom_missense_mutation_dependency` 只使用了 `OmicsSomaticMutations.csv` 中的 `ModelID`、`IsDefaultEntryForModel`、`HugoSymbol`、`VariantInfo` 和 `Hotspot`，没有使用 `LikelyLoF`、driver、pathogenicity、高影响标记或旧 `isDeleterious`。

## 当前项目建议

后续不要尝试把所有新版字段重新压成一个无来源的 `isDeleterious_v2`。应建立多个可并列审查的事件层：

| 事件层 | 推荐依据 | 主要用途 |
|---|---|---|
| `any_coding_event` | 明确选择的 `VariantInfo` / `MolecularConsequence` | 宽口径探索，必须明确包含哪些后果 |
| `official_damaging` | `OmicsSomaticMutationsMatrixDamaging.csv` | 与DepMap 26Q1官方damaging定义一致 |
| `likely_lof` | `LikelyLoF`及相关LoF证据 | 抑癌基因失活和条件依赖分析 |
| `oncogene_high_impact` | `OncogeneHighImpact` | 癌基因高影响事件分析 |
| `tumor_suppressor_high_impact` | `TumorSuppressorHighImpact` | 抑癌基因高影响事件分析 |
| `driver` | `HessDriver`及可追溯的其他driver证据 | 驱动事件分层 |
| `hotspot` | 官方hotspot矩阵或 `Hotspot` | 高频/已知热点事件分析 |
| `pathogenic_missense` | `AMClass`、`AMPathogenicity`及其他预测 | missense敏感性分析，不作为单一真值 |

每层都应记录输入版本、字段规则、缺失值处理、阳性/阴性/未知状态、实际样本量和结果适用边界。跨20Q4与26Q1比较时，应重新按各版本原始变异注释构建统一口径，不能直接比较两个版本已有的deleterious/damaging布尔结果。
