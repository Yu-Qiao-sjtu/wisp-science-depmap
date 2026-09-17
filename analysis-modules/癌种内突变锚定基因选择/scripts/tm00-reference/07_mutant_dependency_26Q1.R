################################################
################################################
### 作者：果子
### 维护者：乔宇（上海交通大学，免疫药理PhD）
### 更新时间：2026-09-09
### 数据迁移：2026-09-08（20Q4/22Q2 旧数据 → 26Q1 新版数据；
###           突变源 CCLE_mutations.csv → OmicsSomaticMutationsMatrixDamaging.csv，
###           字段 isDeleterious → LikelyLoF，详见第一部分字段演进注释）
###           脚本名同步由 07_mutant_dependency_22Q2.R 更名为 _26Q1.R
### 数据修复：2026-09-09 重建全量 cellinfor.rds（2154 行，此前被 01 脚本
###           覆盖为裁剪版 1140 行），coID 由 1140 恢复为 1208（= dep∩mut
###           口径）；Part 5 已用新口径重跑（ARID1A Mut=124/WT=1084）

### ================================================================
### 分析思路（突变 → 基因依赖性 → 合成致死靶点发现）：
###
### 核心问题：某个基因（如 ARID1A）突变的细胞系，
###           对哪些基因（如 HMGCR）更加依赖？
### 生物学意义：肿瘤抑制基因突变后，癌细胞会代偿性依赖其他通路，
###             这些代偿依赖基因就是潜在的治疗靶点（合成致死策略）
###
### 经典案例：PARP 抑制剂治疗 BRCA 突变肿瘤
###   BRCA 突变 → HR 修复缺陷 → 代偿依赖 PARP → PARP 抑制剂有效
### 本脚本案例：ARID1A 突变 → SWI/SNF 复合体缺陷 → 代偿依赖 HMGCR/HMGCS1
###
### ★ 为什么选 ARID1A 和 HMGCR 这一对？
###   直接来源：复现一篇 Cancer Cell 文章的核心图表（逐字稿 02 讲
###   [00:14]：文章研究 ARID1A 突变如何影响 HMGCR 依赖性）——
###   这一对是照着已发表高分文章选的"标准答案"，非随意挑选。
###
###   ARID1A 满足"这类分析的完美基因"全部条件：
###   ① 突变频率高 → Mut 组样本充足：癌症中突变率最高的染色质
###      重塑基因（卵巢透明细胞癌 >50%、子宫内膜癌/胃癌 10~30%），
###      本次运行 Mut_n=124 株（2026-09-09 1208 口径），在全数据集中排得上号
###   ② 典型抑癌基因 → 突变方式为功能丧失：与 Damaging
###      （LikelyLoF）矩阵完美匹配——"刹车被拆"正是它坏掉的方式
###   ③ 本身不可成药 → 合成致死有临床价值：ARID1A 失活无法用
###      药物"修回来"；已验证搭档（EZH2、ARID1B、ATR）均已进入
###      临床试验方向
###   ④ 有已知答案可验证：批量结果中 ARID1B（FDR=7.8e-09）、
###      WRN、DCAF7 被自动捞回 Top 显著 → 证明流程能复现文献
###   选 HMGCR：ARID1A 缺陷细胞代偿依赖胆固醇合成通路
###   （HMGCR 为限速酶）→ 他汀类（老药）可能选择性杀伤
###   ARID1A 突变肿瘤——老药新用，转化价值直观。
###   一句话：ARID1A 是"教科书案例基因"，HMGCR 是"教科书案例
###   答案"，选它们是为了先跑通流程、验证方法，之后换自己的基因
###   （第四部分 mutPlot 与第五部分批量分析就是为此准备的）
###
### ★ 癌种说明：本脚本为泛癌（pan-cancer）分析，癌种未限定
###   每株细胞系仅被自动贴上疾病标签（OncoTree 分类，来自
###   Model.csv 的 OncotreePrimaryDisease），不筛癌种，全部
###   ~1208 株一起进 WT vs Mut 对比（批量分析 124 Mut vs 1084 WT；
###   2026-09-09 修复 cellinfor.rds 被裁剪为 1140 行的问题后，
###   coID 已与 dep∩mut 双交集口径对齐，见头部数据迁移说明）。
###   癌种只影响两处：
###   ① 第二部分散点图：每个组织一个气泡（横坐标=该组织突变组
###      均值，纵坐标=组内差值），统计在组织内做
###   ② 组织准入门槛 filter(Mut>=2, WT>=5)：某组织至少 2 突变、
###      5 野生才显示，防小样本噪声
###   唯一硬编码的癌种在第三部分（子宫内膜癌，因 ARID1A 突变率
###   高、效应直观）；换癌种改那个字符串即可（须为
###   OncotreePrimaryDisease 标准名，可用 table(cellinfor$
###   OncotreePrimaryDisease) 查全部取值）。
###   若需真正限定单一癌种：在 data 构造后加
###     data <- data %>% filter(Tissue == "某癌种")
###   ⚠ 统计学后果：限定后样本量骤降（泛癌 Mut=124，子宫内膜癌
###   内部仅剩 ~15-20 株 Mut），批量 t-test 的 Mut_n>=5 门槛会刷掉
###   绝大多数基因对。正确姿势：泛癌批量筛（发现候选）→
###   单癌种分层验证（第三部分那种图），两层配合，而非一上来就限定
###
### 通俗理解（看的是"细胞"的后果，不是 A 基因自己的后果）：
###   突变造成的后果不体现在细胞外表上，而体现在"细胞离不开谁"上。
###   假想 6 株细胞，问：ARID1A 突变有什么后果？
###
###   ① 分组（来自突变矩阵 mutData）：
###        细胞1/2/3：ARID1A 坏了（Mut）；细胞4/5/6：没坏（WT）
###   ② 问一个具体问题："敲掉 HMGCR，每株细胞会怎样？"
###      DepMap 已用 CRISPR 把每株细胞、每个基因都敲过一遍并打了分
###      （即 geneDependency 矩阵，0~1 概率，越大 = 敲掉后越活不下去）：
###        Mut 组：0.9 / 0.8 / 0.85（死得很惨）→ 均值 0.85
###        WT  组：0.1 / 0.2 / 0.15（基本没事）→ 均值 0.15
###   ③ 差值 Difference = 0.85 - 0.15 = 0.7
###      → ARID1A 坏的细胞敲掉 HMGCR 就活不了；好的细胞无所谓
###      → 反过来读：ARID1A 坏掉后，细胞被迫改用 HMGCR 这条路活命（"上瘾"）
###   ④ 整个脚本 = 把 ③ 对 ~1.7 万个基因各问一遍，
###      差别最大的基因 = 突变细胞赖以活命的基因 = 潜在药靶
###
###   一句话：ARID1A 突变本身不杀细胞，但它逼细胞"上了瘾"——
###           本脚本就是用敲基因实验，把"对什么上瘾"一个一个试出来。
###   （A 基因自己是抑癌基因、不可成药；但它逼出的"瘾"——代偿依赖
###     基因——往往可以成药，这就是绕过"靶点不可成药"的合成致死思路）
###
### ⚠ 方向提醒：本脚本用 Dependency（0~1，越大=越依赖），故
###   Difference = Mut均值 - WT均值 > 0 才表示"突变后更依赖"（第二部分
###   散点图黄色上半区即此意）；切勿沿用 Gene Effect"越负=越依赖"的旧习惯
###
### 分析流程：
###   第一部分：数据预处理（突变矩阵 + 基因依赖性 + 细胞系信息）
###   第二部分：ARID1A 突变 vs HMGCR 依赖性（手动示例 + 散点图）
###   第三部分：提取典型组织做小提琴图（WT vs Mut 对比）
###   第四部分：封装成可复用函数 mutPlot()
###   第五部分：批量分析（全基因组遍历 + 火山图 + 条形图）
### ================================================================

############################################################
## =================== 第一部分：数据预处理 ===================
## 三份数据需要预处理并取交集：突变、基因依赖性、细胞系信息
##
## ★★★ 本部分整体流水线总览（三步一套，缺一不可）★★★
##
##   第一步【备料】：三份数据各自清洗（下面第 1/2/3 小节）
##     每份数据独立加工成统一标准格式：
##       行 = 细胞系（ModelID/ACH-xxx 作行名）× 列 = 基因或注释字段
##     清洗动作：去元数据列 → 设行名 → 清列名去 EntrezID 后缀
##     各自落盘 RDS（一次清洗处处复用，09/10/11 脚本直接 readRDS）
##
##   第二步【摆盘】：第二部分开头从 RDS 秒级加载回三份数据
##
##   第三步【对齐合并】：按行名（ModelID）取三重交集 → coID
##     三份数据覆盖范围不同（突变 1968 / 依赖 1208 / 注释 2154 株），
##     只有交集里的细胞才有完整三份数据；coID 是后续一切分析的
##     细胞系名单——mutData[coID,"ARID1A"]、geneDependency[coID,"HMGCR"]
##     三个向量同序同名同株，可直接配对做 WT vs Mut 对比
##
##   流水线图示：
##     OmicsSomaticMutations...Damaging.csv  CRISPRGeneDependency.csv  Model.csv
##            │①清洗+落盘                    │①清洗+落盘             │①设行名+落盘
##            ▼                              ▼                       ▼
##     mutData_damaging.rds          geneDependency.rds       cellinfor.rds
##            └────────②readRDS 加载────────┴────────②readRDS 加载────┘
##                                       ▼
##                        ③ coID = 三者行名交集（1208 株 = dep∩mut 口径）
##                                       ▼
##                    同一批细胞 × ARID1A 突变状态 × HMGCR 依赖
############################################################

## ---------- 1. 突变信息（新版为损伤性突变矩阵格式）----------
## OmicsSomaticMutationsMatrixDamaging.csv 原始结构（实测，19584 列）：
##   行 = 测序条目（以细胞系 ModelID 为主体，过滤前同细胞系可能多行，~2000 行）
##   前 6 列 = 元数据（实测顺序）：
##     V1(空表头索引) | SequencingID | ModelID | ModelConditionID |
##     IsDefaultEntryForModel | IsDefaultEntryForMC
##     ★ 无 GenomeVersion 列；IsDefaultEntryForModel 在前（与旧推断相反）
##   第 7 列起 = 基因列（19578 个），列名格式 "基因名 (EntrezID)"，
##     如 "ARID1A (8289)"；含 lncRNA/假基因等非编码基因，范围比表达量矩阵宽
##   值 = 0/1/2：0=无损伤性突变，1=杂合损伤，
##   2=近纯合损伤（GT=="1|1" 或 AF 加和≥0.95）；NA=无记录
##
## 通俗理解 1 vs 2（两份拷贝视角）：
##   人有两套染色体，每个基因有两份拷贝（等位基因）：
##   正常：       拷贝1✓ + 拷贝2✓ → 蛋白充足，功能正常
##   杂合损伤(1)：拷贝1✗ + 拷贝2✓ → 坏一份，还剩一半好拷贝
##                （Knudson 二次打击学说的"第一次打击"）
##   近纯合损伤(2)：拷贝1✗ + 拷贝2✗ → 两份全坏，功能彻底丧失
##   判定原理：AF（突变等位基因频率）—只有一份坏时 AF≈0.5，
##   两份全坏时几乎所有 reads 都是突变型（AF≈1）；
##   因测序假计数使 AF 永远达不到精确 1.0，故取加和>0.95 为"近"纯合
##   （或直接测出 GT=="1|1"）
##   本脚本 WT/Mut 分组把 1 和 2 都归入 Mut（只要损伤就算坏，保守不漏；
##   如需严格"完全失活"分组可只取 2，但样本量会变小）
##
## 清理后（本脚本落盘的 mutData_damaging.rds）：
##   行 = 细胞系（ACH-xxx，行名，1968 个）× 列 = 基因符号（纯基因名，19578 个）
##   与 geneDependency 同构（均行=细胞系、列=基因），行名可直接 intersect 对齐；
##   取法：mutData[coID, "ARID1A"] 返回一批细胞系在该基因的突变状态向量
##
## 损伤性判定字段演进（旧版 → 新版）：
##   旧版 CCLE_mutations.csv 长表用 isDeleterious（true/false，仅 MAF
##   功能缺失后果类别白名单，missense 一律 FALSE，官方已废弃）
##   → 中间版 CCLEDeleterious（23Q2 前后）
##   → 现行 26Q1 宽矩阵，由 MAF 长表第 55 列 LikelyLoF 聚合而来：
##     LikelyLoF = True 当满足任一：
##     (1) OncoKB mutation effect 为 "(Likely) Loss-of-function"
##     (2) VEP impact == "HIGH"
##   详见 08 脚本头部注释与 docs/26Q1_Mutation_Pipeline_Documentation.pdf
##
## 注意：本矩阵 ≠ "是否突变"，只是"是否损伤性突变"；
##   BRAF V600E 等热点 missense 不在此矩阵中（需用 MatrixHotspot）
##
## 通俗理解 LikelyLoF（Loss of Function = 功能丧失）：
##   "坏没坏" = 该基因的蛋白功能是否丧失：
##   正常：   基因 → 转录 → 翻译 → 蛋白（有功能，正常工作）
##   坏（Mut）：发生移码/无义/剪接破坏突变 → 蛋白被截短或序列乱码
##             → 造不出有功能的蛋白 → 功能丧失
##   没坏（WT）：基因正常（或仅无害变异）→ 蛋白正常工作
##   典型损伤类型：蛋白提前截短（无义）、序列乱码（移码）、
##   拼不出来（剪接破坏）——蛋白要么缺一大块、要么根本造不出来
##
## 为何研究抑癌基因（如 ARID1A）必须用"功能丧失"定义"坏"：
##   抑癌基因 = 细胞的刹车，其致癌方式只有一种——刹车被拆（功能丧失）；
##   "基因突变"≠"坏"（错义可能只换了个无关紧要的氨基酸，刹车还好好的）；
##   只有功能丧失才是肿瘤发生状态，才会产生代偿依赖 → 合成致死靶点；
##   反之 BRAF/KRAS 等癌基因（油门）是"焊死油门"式功能增强，
##   要用 Hotspot 字段筛，与本矩阵的"功能丧失"是两种完全不同的致癌方式
rm(list = ls())

## 读取突变矩阵
mutData_raw <- data.table::fread("data/OmicsSomaticMutationsMatrixDamaging.csv", data.table = F)

## 新版数据每个细胞系可能有多个测序条目，仅保留默认条目
mutData_raw <- mutData_raw[mutData_raw$IsDefaultEntryForModel == "Yes", ]

## 去除重复的 ModelID（确保每个细胞系只出现一次）
mutData_raw <- mutData_raw[!duplicated(mutData_raw$ModelID), ]

## 将 ModelID 设为行名，方便后续按细胞系 ID 提取
rownames(mutData_raw) <- mutData_raw$ModelID

## 删除前6列元数据（V1, SequencingID, ModelConditionID, ModelID,
## IsDefaultEntryForMC, IsDefaultEntryForModel），只保留基因列
mutData <- mutData_raw[, -(1:6)]

## NA 替换为 0（NA 表示无该基因的突变记录）
mutData[is.na(mutData)] <- 0

## 列名去掉 (EntrezID) 后缀，如 "ARID1A (8289)" → "ARID1A"
colnames(mutData) <- gsub(" \\((\\d+)\\)", "", colnames(mutData))

## 保存处理后的突变矩阵
saveRDS(mutData, file = "TM00/DepMap_TM00/output/mutData_damaging.rds")


## ---------- 2. DepMap 基因 Dependency 数据 ----------
## CRISPRGeneDependency.csv 原始结构（实测，18532 列）：
##   第 1 列 V1 = 无表头索引列（实为 ACH-编号，设行名后删除）
##   第 2 列起 = 基因列（18531 个），列名格式 "基因名 (EntrezID)"，
##     如 "A1BG (1)"（第 127 行去掉后缀）
##   行 = 细胞系（1208 株，覆盖范围比突变矩阵窄，故需 intersect 取交集）
##   值 = 概率（0~1），越高表示越依赖该基因，官方阈值 >0.5 判定为依赖
##
## ★ Dependency vs Gene Effect 辨析（同源兄弟文件，勿混淆）：
##   CRISPRGeneEffect.csv（01 脚本读的 geneEffect）：
##     值 = 效应评分（约 -1.5~+1，拷贝数偏差纠正后的 CERES/Chronos 分）
##     方向：越负 = 越依赖（习惯阈值 < -0.5）
##     用途：定量比较/相关分析（连续值更合适）
##   CRISPRGeneDependency.csv（本处 geneDependency）：
##     值 = 依赖概率（0~1，由 Gene Effect 经贝叶斯概率模型转换而来）
##     方向：越大 = 越依赖（官方阈值 > 0.5）
##     用途：二元判断"依赖/不依赖"、依赖计数
##   ⚠ 两者方向相反（负 vs 正），本脚本用 Dependency 画散点/小提琴时
##   Y 轴解读应为"值越大=越依赖"，不能沿用 Gene Effect 的"越负=越依赖"；
##   若需定量比较效应大小，换用 geneEffect（01 脚本已落盘 depmap_geneEffect.rds）
rm(list = ls())

## 读取基因依赖性矩阵（18532 列大文件，fread 比 read.csv 快；
## data.table=F 返回普通 data.frame，兼容后续 dplyr/ggplot2）
geneDependency <- data.table::fread("data/CRISPRGeneDependency.csv", data.table = F)

## 修改列名，去掉 (EntrezID) 后缀："A1BG (1)" → "A1BG"、"ARID1A (8289)" → "ARID1A"
## 必须做：后续用 geneDependency[coID, "HMGCR"] 按基因名取值，列名带后缀会查不到
colnames(geneDependency) <- gsub("\\s+\\(\\d+\\)", "", colnames(geneDependency))

## 第 1 列（V1，即 ACH-xxx 细胞系编号）设为行名后删除——两步缺一不可：
##   ① 设行名：后续按细胞系 ID 取值/取交集全靠行名
##   ② 删列：不删则矩阵里混一列字符型 ID，整个 data.frame 会变字符型，无法数值运算
rownames(geneDependency) <- geneDependency[, 1]
geneDependency <- geneDependency[, -1]

## 保存处理后的基因依赖性矩阵（行=ACH 细胞系行名、列=纯基因名的 0~1 概率矩阵）
## 之后 07 后半部分及 09/10/11 脚本均 readRDS 秒级加载，不必重复读大 CSV；
## 清理后与 mutData_damaging.rds 同构（均行=细胞系、列=基因），行名可直接 intersect 对齐
saveRDS(geneDependency, file = "TM00/DepMap_TM00/output/geneDependency.rds")


## ---------- 3. 细胞系信息 ----------
## Model.csv：包含每个细胞系的元数据（疾病类型、组织来源等）
rm(list = ls())

## 读取细胞系信息表
cellinfor <- data.table::fread("data/Model.csv", data.table = F)

## 将 ModelID（ACH-编号）设为行名
rownames(cellinfor) <- cellinfor$ModelID

## 保存处理后的细胞系信息
saveRDS(cellinfor, file = "TM00/DepMap_TM00/output/cellinfor.rds")


###############################################################
###############################################################
## =================== 第二部分：ARID1A 突变 vs HMGCR 依赖性 ===================
## 经典案例：ARID1A（SWI/SNF 复合体亚基）突变的癌细胞，
##           对胆固醇合成通路基因 HMGCR 的依赖性显著升高
###############################################################
rm(list = ls())

## ---- 加载R包和数据 ----
library(dplyr)     ## 数据操作
library(tidyr)     ## 宽长格式转换
library(tibble)    ## 行名/列名操作

## 读取三份预处理好的数据
## 【流水线第②步·摆盘】第一部分已各自清洗落盘，此处 readRDS 秒级加载；
## 三份均为统一格式：行名 = ModelID（ACH-xxx）、列 = 基因或注释字段
mutData <- readRDS(file = "TM00/DepMap_TM00/output/mutData_damaging.rds")
geneDependency <- readRDS(file = "TM00/DepMap_TM00/output/geneDependency.rds")
cellinfor <- readRDS(file = "TM00/DepMap_TM00/output/cellinfor.rds")

## 三份数据取交集——只有同时有突变、依赖性、细胞系信息的才纳入分析
## 【流水线第③步·对齐合并】真正意义上的"合并"：不拼大表，而是拿到
## 三份数据共同的细胞系名单 coID；后续所有取值都用 mutData[coID, ...]
## 这种子集化方式，保证三个向量同序同名同株（详见第一部分总览注释）
coID <- intersect(rownames(geneDependency), rownames(cellinfor)) %>%
  intersect(rownames(mutData))

## ---- 整理作图数据 ----
## ARID1A 突变状态（WT=野生型，Mut=突变型） × HMGCR 依赖性评分 × 组织类型
data <- data.frame(
  DepmapID = coID,                                          ## 细胞系 ID
  Tissue = cellinfor[coID, "OncotreePrimaryDisease"],       ## 组织/疾病类型
  Mutation = ifelse(mutData[coID, "ARID1A"] == 0, "WT", "Mut"),  ## ARID1A 突变状态
  Dependency = geneDependency[coID, "HMGCR"]                ## HMGCR 依赖性评分
)

## ---- 按组织类型统计突变/野生型数量 ----
## 筛选条件：每个组织至少有 2 个突变样本和 5 个野生型样本
## 目的：排除样本量太小的组织，保证统计可靠性
TissueStatus <- data %>%
  select(Tissue, Mutation) %>%                              ## 只保留组织和突变状态
  group_by(Tissue, Mutation) %>%                            ## 按组织+突变状态分组计数
  summarise(n = n()) %>%
  ungroup() %>%
  pivot_wider(names_from = "Mutation",                      ## 转为宽格式：WT和Mut各一列
              values_from = n,
              values_fill = 0) %>%
  filter(Mut >= 2, WT >= 5) %>%                             ## 过滤样本量不足的组织
  mutate(Sum = Mut + WT) %>%                                ## 该组织总样本数
  mutate(Mut_Number = sum(Mut), WT_Number = sum(WT))        ## 全局突变/野生型总数

## 只保留符合条件的组织的数据
mydata <- data %>%
  filter(Tissue %in% TissueStatus$Tissue)

## ---- 计算每个组织的两个指标 ----
## Sensitivity：突变型细胞系的平均依赖性（Dependency 0~1 口径：越大=越依赖=越敏感）
Mean_Dep <- mydata %>%
  filter(Mutation == "Mut") %>%                             ## 只看突变型
  group_by(Tissue) %>%
  summarise(Sensitivity = mean(Dependency))                 ## 突变型的平均依赖性

## Difference：突变型 - 野生型的依赖性差值（Dependency 口径下 >0 = 突变后更依赖，即散点图黄色上半区）
Diff_Dep <- mydata %>%
  group_by(Tissue) %>%
  summarize(Difference = mean(Dependency[Mutation == "Mut"]) -
                          mean(Dependency[Mutation == "WT"]))

## 合并所有作图数据（每个组织一行）
PlotData <- merge(Mean_Dep, Diff_Dep, by = "Tissue") %>%
  merge(TissueStatus, by = "Tissue")


## ---- 作图：组织级散点图 ----
library(ggplot2)
library(ggrepel)    ## 标签自动避让
library(stringr)    ## 文本换行
library(ggfun)      ## 圆角图例背景

p1 <- ggplot(PlotData, aes(x = Sensitivity, y = Difference)) +
  ## 气泡点：大小=样本数，颜色=绿色
  geom_point(aes(size = Sum), shape = 21, color = "black", fill = "green", alpha = 0.6, stroke = 1.5) +
  ## 组织名称标签，自动避让防重叠
  geom_text_repel(aes(label = Tissue), vjust = 1.5, hjust = 1.5) +
  ## 上半区黄色高亮：Difference > 0（突变后更依赖=潜在合成致死靶点）
  annotate("rect", xmin = -Inf, xmax = Inf, ymin = 0, ymax = Inf,
           fill = "yellow", alpha = 0.1, colour = "green", linetype = "dashed", linewidth = 1) +
  ## 轴标签和标题（含突变/野生型总数）
  labs(x = "Sensitivity Score for Cell Lines with Mutations",
       y = "Mutant vs. Wildtype Sensitivity",
       title = paste0("Mutant (n=", unique(PlotData$Mut_Number),
                      " cell lines) vs. Wildtype (n=",
                      unique(PlotData$WT_Number), " cell lines)")) +
  theme_bw() +
  ## 图例标题换行显示
  guides(size = guide_legend(title = str_wrap("Number of cells", width = 8))) +
  theme(panel.grid.major = element_line(linetype = "dashed"),
        panel.grid.minor = element_line(linetype = "dashed", size = 0.5),
        panel.background = element_blank(),
        legend.position = c(.95, .05),                      ## 图例放右下角
        legend.justification = c("right", "bottom"),
        legend.box.just = "right",
        legend.margin = margin(4, 4, 4, 4),
        legend.background = element_roundrect(color = "#808080", linetype = 1)  ## 圆角图例
  )

## 打印到屏幕 + 保存为 PNG
print(p1)
ggsave("TM00/DepMap_TM00/output/07_ARID1A_HMGCR.png", p1, width = 8, height = 6, dpi = 300)
cat("HMGCR 散点图已保存\n")


###################################################
## =================== 第三部分：典型组织小提琴图 ===================
## 从散点图中选出差异最大的组织，单独做 WT vs Mut 的小提琴图
## 直观展示：在该组织中，ARID1A 突变型对 HMGCS1 的依赖性是否显著高于野生型
###################################################

library(ggplot2)

## 提取子宫内膜癌的数据（该组织中 ARID1A 突变率高，效应明显）
## 【本脚唯一硬编码的癌种入口】换癌种改此字符串即可；
##   须为 OncotreePrimaryDisease 标准名（如 "Lung Adenocarcinoma"、
##   "Breast Invasive Carcinoma"），可用 table(cellinfor$
##   OncotreePrimaryDisease) 查全部取值；详见头部"癌种说明"注释
df = mydata[mydata$Tissue=="Endometrial Carcinoma",]

## 设置因子顺序和标签（含样本量信息）
df$Mutation <- factor(df$Mutation, levels = c("WT", "Mut"),
                      labels = c("ARID1A(wt) \n (n=11)", "ARID1A(mut) \n (n=15)"))

## 小提琴图 + 点图叠加
p3 <- ggplot(df, aes(x = Mutation, y = Dependency, fill = Mutation)) +
  geom_violin(alpha = 0.5) +                                ## 小提琴图：展示分布形状
  geom_dotplot(binaxis = "y",                               ## 点图叠加：展示每个样本
               stackdir = "center",
               dotsize = 0.5) +
  labs(y = "Dependency Score")+                             ## Y轴=依赖性评分
  theme_bw() +
  theme(legend.position = "none")                           ## 隐藏图例（X轴已区分）

print(p3)
ggsave("TM00/DepMap_TM00/output/07_ARID1A_HMGCS1_violin.png", p3, width = 5, height = 5, dpi = 300)
cat("小提琴图已保存\n")


###################################################
## =================== 第四部分：封装成可复用函数 mutPlot() ===================
## 前面手动写了一遍 HMGCR 的分析，现在封装成函数
## 以后换任何突变基因×靶基因对都能一键出图
##
## 参数：
##   mutGene    突变基因名（如 "ARID1A"）
##   targerGene 靶基因名（如 "HMGCR"）
##
## 依赖的全局变量：coID, mutData, geneDependency, cellinfor
###################################################

mutPlot <- function(mutGene, targerGene){
  ## ---- 构造分析数据（和第二部分完全一样的逻辑，只是基因名变为参数）----
  data <- data.frame(
    DepmapID = coID,
    Tissue = cellinfor[coID, "OncotreePrimaryDisease"],
    Mutation = ifelse(mutData[coID, mutGene] == 0, "WT", "Mut"),    ## 参数化的突变基因
    Dependency = geneDependency[coID, targerGene]                   ## 参数化的靶基因
  )

  ## ---- 组织筛选（Mut>=2, WT>=5）----
  TissueStatus <- data %>%
    select(Tissue, Mutation) %>%
    group_by(Tissue, Mutation) %>%
    summarise(n = n()) %>%
    ungroup() %>%
    pivot_wider(names_from = "Mutation",
                values_from = n, values_fill = 0) %>%
    filter(Mut >= 2, WT >= 5) %>%
    mutate(Sum = Mut + WT) %>%
    mutate(Mut_Number = sum(Mut), WT_Number = sum(WT))

  mydata <- data %>%
    filter(Tissue %in% TissueStatus$Tissue)

  ## ---- 计算指标 ----
  Mean_Dep <- mydata %>%
    filter(Mutation == "Mut") %>%
    group_by(Tissue) %>%
    summarise(Sensitivity = mean(Dependency))

  Diff_Dep <- mydata %>%
    group_by(Tissue) %>%
    summarize(Difference = mean(Dependency[Mutation == "Mut"]) - mean(Dependency[Mutation == "WT"]))

  PlotData <- merge(Mean_Dep, Diff_Dep, by = "Tissue")
  PlotData <- merge(PlotData, TissueStatus, by = "Tissue")

  ## ---- 作图（标题自动包含基因名）----
  library(ggplot2)
  library(ggrepel)
  library(stringr)

  p <- ggplot(PlotData, aes(x = Sensitivity, y = Difference)) +
    geom_point(aes(size = Sum), shape = 21, color = "black", fill = "green", alpha = 0.6, stroke = 1.5) +
    geom_text_repel(aes(label = Tissue), vjust = 1.5, hjust = 1.5) +
    annotate("rect", xmin = -Inf, xmax = Inf, ymin = 0, ymax = Inf,
             fill = "yellow", alpha = 0.1, colour = "green", linetype = "dashed", size = 1) +
    labs(x = "Sensitivity Score for Cell Lines with Mutations",
         y = "Mutant vs. Wildtype Sensitivity",
         title = paste0(mutGene, " Mutant (n=", unique(PlotData$Mut_Number),
                        ") vs. Wildtype (n=", unique(PlotData$WT_Number), ") → ", targerGene)) +
    theme_bw() +
    guides(size = guide_legend(title = str_wrap("Number of cells", width = 8))) +
    theme(panel.grid.major = element_line(linetype = "dashed"),
          panel.grid.minor = element_line(linetype = "dashed", size = 0.5),
          panel.background = element_blank(),
          legend.position = c(.95, .05),
          legend.justification = c("right", "bottom"),
          legend.box.just = "right",
          legend.margin = margin(4, 4, 4, 4),
          legend.background = element_roundrect(color = "#808080", linetype = 1)
    )
  print(p)
  ## 文件名自动按"突变基因_靶基因"命名，避免覆盖
  ggsave(paste0("TM00/DepMap_TM00/output/07_", mutGene, "_", targerGene, ".png"), p, width = 8, height = 6, dpi = 300)
  cat(mutGene, "_", targerGene, " 图已保存\n", sep = "")

  return(p)
}


## ---- 函数调用示例 ----
mutPlot(mutGene = "ARID1A", targerGene = "HMGCR")           ## ARID1A 突变 → HMGCR 依赖
mutPlot(mutGene = "ARID1A", targerGene = "HMGCS1")          ## 同通路另一个酶（替代手动写第二遍）
mutPlot(mutGene = "TP53", targerGene = "FOXA1")             ## 换一对基因


### ================================================================
### ================== 第五部分：批量分析 ==========================
### ================================================================
### 方向1：已知突变基因（如 ARID1A），找哪些靶基因受影响最大
###   → 遍历全部 ~1.7 万个基因的依赖性，计算突变 vs 野生型差异
###   生物学意义：ARID1A 突变的癌细胞代偿依赖哪些基因？
###               这些基因就是潜在的合成致死治疗靶点
###
### 方向2：已知感兴趣的靶基因（如 HMGCR），找哪个突变基因影响它
###   → 遍历全部 ~1.7 万个基因的突变状态，计算对靶基因依赖性的差异
###   生物学意义：哪些突变导致了 HMGCR 依赖？
###               帮助理解 HMGCR 依赖的遗传基础
###
### 批量指标：Difference（突变型均值 - 野生型均值）+ Welch's t-test p 值
###   ⚠ Dependency（0~1）口径下：Difference > 0 = 突变型更依赖该基因 = 合成致死候选
###   （切勿沿用 Gene Effect"越负=越依赖"的旧习惯，见第一部分辨析注释）
### ================================================================

## =================== 方向1：已知突变基因，批量找靶基因 ===================
## 输入：ARID1A 突变 → 遍历全部基因的依赖性
rm(list = ls())
library(dplyr)

## ---- 重新加载数据（rm 清空了前面的变量）----
mutData <- readRDS(file = "TM00/DepMap_TM00/output/mutData_damaging.rds")
geneDependency <- readRDS(file = "TM00/DepMap_TM00/output/geneDependency.rds")
cellinfor <- readRDS(file = "TM00/DepMap_TM00/output/cellinfor.rds")
coID <- intersect(rownames(geneDependency), rownames(cellinfor)) %>%
  intersect(rownames(mutData))

## ---- 批量计算函数 ----
## 输入：一个突变基因 + 一个靶基因，输出：Difference + p 值
## 不画图，只返回数值，方便批量调用
mutStat <- function(mutGene, targetGene, mutData, geneDependency, coID) {

  ## 1. 提取突变状态向量（WT=野生型，Mut=突变型）
  mutVec <- ifelse(mutData[coID, mutGene] == 0, "WT", "Mut")

  ## 2. 提取依赖性评分向量
  depVec <- geneDependency[coID, targetGene]

  ## 3. 样本量检查：突变型至少 5 个，野生型至少 10 个，否则跳过
  if (sum(mutVec == "Mut", na.rm = TRUE) < 5 || sum(mutVec == "WT", na.rm = TRUE) < 10) {
    return(data.frame(MutGene = mutGene, TargetGene = targetGene,
                      Mut_n = sum(mutVec == "Mut", na.rm = TRUE),
                      WT_n = sum(mutVec == "WT", na.rm = TRUE),
                      Difference = NA, pvalue = NA))
  }

  ## 4. Welch's t-test 比较突变型 vs 野生型的依赖性
  ##    Welch's 不假设等方差，比标准 t-test 更稳健
  tt <- tryCatch(t.test(depVec[mutVec == "Mut"], depVec[mutVec == "WT"]),
                 error = function(e) NULL)

  ## 5. t-test 失败时返回 NA
  if (is.null(tt)) {
    return(data.frame(MutGene = mutGene, TargetGene = targetGene,
                      Mut_n = sum(mutVec == "Mut", na.rm = TRUE),
                      WT_n = sum(mutVec == "WT", na.rm = TRUE),
                      Difference = NA, pvalue = NA))
  }

  ## 6. 返回统计结果
  data.frame(
    MutGene    = mutGene,                                         ## 突变基因名
    TargetGene = targetGene,                                      ## 靶基因名
    Mut_n      = sum(mutVec == "Mut", na.rm = TRUE),              ## 突变型样本数
    WT_n       = sum(mutVec == "WT", na.rm = TRUE),               ## 野生型样本数
    Difference = mean(depVec[mutVec == "Mut"], na.rm = TRUE) -    ## 差值 = 突变均值 - 野生均值
                 mean(depVec[mutVec == "WT"], na.rm = TRUE),       ## Dependency 口径下 >0 = 突变后更依赖
    pvalue     = tt$p.value                                        ## t-test p 值
  )
}

## ---- 方向1：固定突变基因 ARID1A，遍历所有靶基因 ----
mutGene <- "ARID1A"                          ## 固定突变基因
all_targets <- colnames(geneDependency)       ## 全部 ~1.7 万个靶基因

## 进度条（1.7 万次 t-test 需要几分钟）
pb <- txtProgressBar(min = 0, max = length(all_targets), style = 3)
result1 <- vector("list", length(all_targets))  ## 预分配 list 提速

## 逐个靶基因计算 ARID1A 突变 vs 野生型的依赖性差异
for (i in seq_along(all_targets)) {
  result1[[i]] <- mutStat(mutGene, all_targets[i], mutData, geneDependency, coID)
  setTxtProgressBar(pb, i)                   ## 更新进度条
}
close(pb)

## 合并所有结果为一个 data.frame
result1_df <- do.call(rbind, result1)        ## list → data.frame
result1_df$FDR <- p.adjust(result1_df$pvalue, method = "BH")  ## BH 校正多重检验
result1_df <- result1_df[order(result1_df$Difference, decreasing = TRUE), ]  ## 按 Difference 降序（最正=突变后最依赖在最前）

cat("\n方向1：ARID1A 突变后，最依赖的 Top20 靶基因（Dependency 口径：Difference 最正）：\n")
print(head(result1_df[!is.na(result1_df$Difference), ], 20))  ## 打印 Top20

saveRDS(result1_df, file = "TM00/DepMap_TM00/output/ARID1A_batch_target_screening.rds")
cat("方向1 结果已保存\n")


## =================== 方向2：已知靶基因，批量找突变基因 ===================
## 反向问题：固定 HMGCR 依赖性，遍历所有突变基因
targetGene <- "HMGCR"                          ## 固定靶基因
all_mutations <- colnames(mutData)              ## 全部 ~1.7 万个突变基因

pb <- txtProgressBar(min = 0, max = length(all_mutations), style = 3)
result2 <- vector("list", length(all_mutations))

## 逐个突变基因计算对 HMGCR 依赖性的影响
for (i in seq_along(all_mutations)) {
  result2[[i]] <- mutStat(all_mutations[i], targetGene, mutData, geneDependency, coID)
  setTxtProgressBar(pb, i)
}
close(pb)

## 合并结果
result2_df <- do.call(rbind, result2)
result2_df$FDR <- p.adjust(result2_df$pvalue, method = "BH")
result2_df <- result2_df[order(result2_df$Difference, decreasing = TRUE), ]  ## 降序：Difference 最正=该突变最能增加 HMGCR 依赖

cat("\n方向2：最能增加 HMGCR 依赖性的突变基因 Top20（Difference 最正）：\n")
print(head(result2_df[!is.na(result2_df$Difference), ], 20))

saveRDS(result2_df, file = "TM00/DepMap_TM00/output/HMGCR_batch_mutation_screening.rds")
cat("方向2 结果已保存\n")


## =================== 可视化1：火山图 ===================
## X 轴 = Difference（效应大小），Y 轴 = -log10(FDR)（统计显著性）
## Dependency 口径下右上角（Difference 正 + FDR 显著）= 合成致死候选
## ⚠ 注意：火山图副标题代码写的是"Negative = ..."（沿用了 Gene Effect
##   旧习惯），与 Dependency 方向相反，解读时以"正=候选"为准
library(ggplot2)

## 去掉 NA 行，定义显著性分组
volcano_data <- result1_df[!is.na(result1_df$Difference), ]
volcano_data$significance <- ifelse(volcano_data$FDR < 0.05 & abs(volcano_data$Difference) > 0.05,
                                    "Significant", "NS")

## 火山图：红点 = 显著，灰点 = 不显著
p_volcano <- ggplot(volcano_data, aes(x = Difference, y = -log10(FDR))) +
  geom_point(aes(color = significance), alpha = 0.5, size = 1) +    ## 散点
  scale_color_manual(values = c("NS" = "grey", "Significant" = "red")) + ## 颜色
  geom_vline(xintercept = c(-0.05, 0.05), linetype = "dashed", color = "blue") + ## 效应量阈值线
  geom_hline(yintercept = -log10(0.05), linetype = "dashed", color = "blue") +  ## FDR 阈值线
  labs(x = "Difference (Mut - WT Dependency)",                       ## X 轴标签
       y = "-log10(FDR)",                                            ## Y 轴标签
       title = "ARID1A Mutation → Target Gene Dependency",           ## 标题
       subtitle = "Positive = Synthetic Lethality Candidate") +     ## 副标题（Dependency 口径：正=候选）
  theme_bw()
print(p_volcano)
ggsave("TM00/DepMap_TM00/output/07_ARID1A_volcano.png", p_volcano, width = 8, height = 6, dpi = 300)
cat("火山图已保存\n")


## =================== 可视化2：Top20 条形图 ===================
## 取 Difference 最正的 20 个基因（已修正：Dependency 口径下正=突变后更依赖
## =合成致死候选；旧代码 order 升序+head() 取最负端是 Gene Effect 旧习惯）
top20 <- head(volcano_data[order(volcano_data$Difference, decreasing = TRUE), ], 20)

p_bar <- ggplot(top20, aes(x = reorder(TargetGene, Difference), y = Difference)) +
  geom_bar(stat = "identity", fill = "steelblue") +  ## 柱子
  coord_flip() +                                     ## 翻转坐标轴（横向条形图）
  labs(x = "", y = "Difference (Mut - WT Dependency)",
       title = "Top20 Synthetic Lethality Candidates\n(ARID1A Mutation, ranked by Difference)") +
  theme_bw()
print(p_bar)
ggsave("TM00/DepMap_TM00/output/07_ARID1A_top20_bar.png", p_bar, width = 7, height = 6, dpi = 300)
cat("条形图已保存\n")
