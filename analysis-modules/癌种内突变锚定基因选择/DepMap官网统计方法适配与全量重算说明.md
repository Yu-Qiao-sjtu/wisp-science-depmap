# DepMap 官网统计方法适配与全量重算说明

## 结论

本次修正覆盖整个突变—基因依赖模块，不限于 TP53。新版结果包括：

1. 每个癌种内全部达到可检验门槛的 Damaging/Hotspot 锚点；
2. 泛癌 Hotspot—全基因依赖；
3. 指定 `ProteinChange` 的按需分析接口及乳腺癌 `PIK3CA p.H1047R` 验证实例。

默认正式结果改为 `CRISPRGeneEffect`。`CRISPRGeneDependency` 概率保留为“依赖/非依赖”比例和优势比的辅助指标，旧的概率连续差异结果继续保留供追溯。

## 官网实际采用的方法

依据 DepMap Portal 当前公开源码（审计提交 `029c20c`）：

- Context Explorer 对连续 `Chronos CRISPR Gene Effect` 做双侧独立样本 t 检验；
- 源码实际参数是 `equal_var=True`，因此是等方差/合并方差 t 检验。源码附近的“Welch”注释与实际参数不一致，本模块以可执行参数为准；
- 每组对每个靶基因至少 5 个非缺失值；
- 固定一个比较后，对所有候选靶基因的 p 值做 Benjamini–Hochberg 校正；
- 只考虑在至少 3 个、至多 95% CRISPR 模型中 `CRISPRGeneDependency > 0.5` 的基因，官网还会补回内部 TDA 表标注的 strongly selective 基因；
- 官网界面默认命中门槛为：FDR≤0.10、平均 Gene Effect 差值小于 -0.25、组内依赖比例≥0.10。

公开依据：

- [Context Explorer 统计实现：分组门槛、t 检验、BH 与效应量](https://github.com/broadinstitute/depmap-portal/blob/029c20c578a6550af0abc6de5934b8b79be77213/pipeline/preprocessing-pipeline/context_explorer/get_context_analysis.py#L20)
- [Context Explorer 默认筛选值](https://github.com/broadinstitute/depmap-portal/blob/029c20c578a6550af0abc6de5934b8b79be77213/frontend/packages/portal-frontend/src/contextExplorer/json/geneDepFilters.json#L1)
- [DepMap Genetic Dependencies FAQ](https://forum.depmap.org/t/depmap-genetic-dependencies-faq/131)
- [Chronos 方法论文](https://pubmed.ncbi.nlm.nih.gov/34930405/)

## 本模块如何适配

官网 Context Explorer 的 in-group 是癌种/亚型，out-group 是其他模型。本模块保留相同的结局指标、检验、完整病例门槛、BH 校正和默认命中规则，只替换分组定义：

- in-group：同一癌种中指定突变矩阵值 `>0` 的模型；
- out-group：同一癌种中同一突变矩阵明确为 `0` 的模型；
- 缺失：排除，不能当作未突变；
- `delta_gene_effect = mean(Mut) - mean(matrix-negative)`；负值表示突变组对该敲除靶点更依赖。

`matrix-negative` 是发布矩阵中的分析对照状态，不代表所有位点都经过无限深度检测，也不能直接写成临床患者的绝对野生型。

## 06 锚点选择与正式检验的关系

06 脚本负责建立“在某癌种中有哪些突变锚点有足够样本”的菜单，不负责证明依赖差异显著。正式分析继承卡片中的事件定义与优先级：

- TSG 与 Damaging 匹配时列为优先锚点；
- 癌基因与 Hotspot 匹配时列为优先锚点；
- 双角色基因可分别保留 Damaging 和 Hotspot；
- 角色未分类或角色不匹配的严格锚点保留为机制复核层；
- 探索锚点保留，但必须显示较低证据等级。

因此，“TP53 突变”默认读取 TP53–Damaging；TP53–Hotspot 是另一个明确标注的补充事件，二者不能混成一个结论。

## 旧版发现的系统性问题

| 问题 | 旧版行为 | 新版处理 |
|---|---|---|
| 主指标 | 把 Dependency 概率当连续值比较，常在核心必需基因处饱和 | 用连续 Gene Effect 检验；概率只做依赖比例 |
| t 检验 | Welch t 检验 | 按官网可执行源码使用 `equal_var=True` |
| 样本门槛 | 固定 Mut≥5、WT≥10，导致 TP53–Damaging 肝癌 18/7 被删除 | 每个靶点两组各≥5，与官网一致 |
| 锚点类型 | Damaging 与 Hotspot 都可能进入，回答时易混为“该基因突变” | 保留事件标签，并按角色匹配结果作为默认解释 |
| 显著性 | 只看 raw p 或只看差值会产生大量假阳性 | 固定锚点后对全部靶点做 BH |
| 效应方向 | 概率差正值和 Gene Effect 差负值含义相反 | 统一显示指标、方向和组均值 |
| 泛癌分析 | 可能受癌种构成差异混杂 | 标为候选发现层，要求癌种内复核或协变量模型 |
| “WT”措辞 | 容易被误解为确定的生物学野生型 | 结果列与说明使用 mutation-matrix-negative |

## 全量结果规模

- 癌种内：30 个癌种目录，23 个癌种存在可检验锚点；共 1,089 个“癌种×锚点×事件类型”，9,578,653 个有效锚点—靶点检验。
- 官网默认规则命中 5,843 对；其中 FDR≤0.05 的严格命中 3,268 对。
- 泛癌 Hotspot：58 个事件，513,208 个有效检验；官网默认规则命中 792 对，严格命中 618 对。
- 乳腺癌 `PIK3CA p.H1047R` 实例：Mut=10、矩阵阴性对照=43、8,924 个有效检验；无官网默认规则命中。

命中数量不是独立机制数量。同一锚点可命中多个相关靶点，同一通路内也可能存在相关性；结果需要结合效应大小、依赖比例、共突变、拷贝数、表达量和实验验证判断。

## 肝癌 TP53 的修正结果

TP53–Damaging 是默认事件，实际 Mut=18、矩阵阴性对照=7，达到官网两组各≥5的计算门槛。共检验 8,860 个靶基因，没有靶点同时满足 FDR≤0.10、`delta_gene_effect < -0.25`、Mut 组依赖比例≥0.10。

最低 FDR 的 SCD 差异方向为正（Mut 均值 -0.543，对照均值 -1.156，差值 +0.613，FDR 0.0356），表示对照组更依赖 SCD，不能回答成“TP53 突变组最依赖 SCD”。MIB1 在 TP53–Damaging 中差值为 -0.373，但 FDR 0.690，不构成全基因组校正后的证据。旧版 MIB1 结论来自 TP53–Hotspot 与 Dependency 概率，不能替代 TP53–Damaging 的正式结果。

肝癌模块还有 TERT–Hotspot、TP53–Hotspot、LINC02900–Damaging、AXIN1–Damaging、TTN–Damaging 等可检验事件，但当前均无官网默认命中。它们是不同事件定义，不能合并为一个“肝癌突变结果”。

## 结果读取规则

用户问“某突变基因在某癌种依赖哪些基因”时：

1. 识别癌种、锚点基因和显式事件类型；
2. 未指定事件类型时，先按角色匹配选择默认事件并说明；
3. 读取癌种内 `anchor_summary.csv` 与 `top_dependency_hits.csv`；
4. 优先返回官网默认命中；无命中时明确写“未发现达到当前全基因组校正与效应门槛的差异依赖”，可附探索性结果，但不能把它写成显著依赖；
5. 用户在 DepMap 语境中说“患者”时，解释为相应癌种的 DepMap 细胞系分组；只有明确要求 TCGA、生存、疗效或临床队列时才切换为患者数据任务。

## 结果目录

- `downstream_dependency/04_depmap_official_gene_effect_v2/01_lineage_all_anchors/`：癌种索引和靶基因过滤审计；
- `by_cancer/<癌种>/dependency_analysis/depmap_official_gene_effect_v2/`：每个癌种的独立正式结果；
- `downstream_dependency/04_depmap_official_gene_effect_v2/02_hotspot_pan_cancer/`：泛癌 Hotspot；
- `downstream_dependency/04_depmap_official_gene_effect_v2/03_protein_change_on_demand/`：按需位点分析。

服务器绝对路径不写入文档、清单或用户结果，只使用模块相对路径。
