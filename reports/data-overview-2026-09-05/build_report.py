import json
from pathlib import Path
from collections import Counter

OUT = Path(__file__).parent
data = json.loads((OUT / 'server-evidence.json').read_text(encoding='utf-8-sig'))
local = Path('D:/New-PHD/depmap_0823/analysis/depmap-kb')
# Each entry links the observed output family to its builder and scientific meaning.
mapping = {
'lineage_custom_missense_mutation_dependency': ('15_build_lineage_somatic_event_dependency.R', '体细胞突变长表 + Gene Effect + Model 癌种', '癌种内自定义突变事件对全部 18,531 靶基因的 Welch 检验；Mut≥3、WT≥5；每事件 BH。custom_missense 包含多种后果，并非仅错义突变。'),
'cnv_amplification_dependency': ('09_build_global_cnv_dependency.R', 'OmicsCNGeneMC_WES + ModelCondition + Gene Effect', '33,320 个扩增基因 × 18,531 靶基因；795 个共同模型；阈值 log2 CN >1.58，扩增≥5、对照≥10；Welch、每扩增基因 BH。'),
'custom_missense_mutation_dependency': ('10_build_somatic_event_dependency.R', 'OmicsSomaticMutations.csv + Gene Effect', '17,391 个自定义事件基因 × 18,531 靶基因；1,208 模型；突变≥5、对照≥10；保留全量统计 block。'),
'damaging_mutation_dependency': ('06_build_global_mutation_dependency.R', 'OmicsSomaticMutationsMatrixDamaging.csv + Gene Effect', '5,590 个 damaging 事件基因 × 18,531 靶基因；1,208 模型；差值为突变组减对照组，负值表示更强依赖。'),
'lineage_damaging_mutation_dependency': ('07_build_lineage_mutation_dependency.R', 'Damaging 矩阵 + Gene Effect + Model', '癌种内 damaging 事件—靶基因全量检验；按癌种和源基因分块。'),
'hotspot_mutation_dependency': ('10_build_somatic_event_dependency.R', '体细胞突变长表 Hotspot==TRUE + Gene Effect', '58 个热点事件基因 × 18,531 靶基因；1,208 模型；与 custom_missense 通过 --event 区分。'),
'lineage_hotspot_mutation_dependency': ('15_build_lineage_somatic_event_dependency.R', 'Hotspot 事件 + Gene Effect + Model', '癌种内热点突变—依赖性全量检验。'),
'effect_correlation': ('04_build_global_correlation_matrices.R', 'CRISPRGeneEffect.csv', '1,208 模型、18,531 基因的 Pearson 共依赖矩阵；145 blocks，保存实际共同样本数 pair_n。'),
'expression_correlation': ('04_build_global_correlation_matrices.R', 'OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv', '1,719 样本、19,215 基因的 Pearson 共表达矩阵；151 blocks。'),
'expression_dependency': ('05_build_expression_dependency_matrix.R', '表达矩阵 + CRISPRGeneEffect.csv', '1,140 共同模型；19,215 表达基因 × 18,531 依赖靶基因；负相关代表高表达伴随更强依赖。'),
'lineage_sparse_networks': ('17_build_lineage_sparse_networks.R;22_finalize_lineage_networks_server.R', '表达 + Gene Effect + Model', '癌种内共表达、共依赖、表达—依赖网络；74 个 complete 子 manifest。全靶基因计算后，每源保留正/负各 Top100，另生成互惠配对。'),
'lineage_cnv_amplification_dependency': ('23_build_lineage_cnv_sparse.R', 'CNV + Gene Effect + Model', '癌种内扩增—依赖性；20 个 complete、14 个样本不足；每源正/负各 Top100，非完整统计矩阵存储。'),
'prism_auc_effect_correlation': ('13_build_prism_feature_correlations.R', 'PRISM_Repurposing_AUC_Matrix.csv + Gene Effect', '1,482 药物 × 18,531 基因；577 共同模型；Pearson、每药跨特征 BH。'),
'prism_auc_expression_correlation': ('13_build_prism_feature_correlations.R', 'PRISM AUC + 表达', '1,482 药物 × 19,215 表达基因；711 共同模型。'),
'prism_auc_cnv_correlation': ('13_build_prism_feature_correlations.R', 'PRISM AUC + CNV', '1,482 药物 × 37,605 CNV 基因；572 共同模型。'),
'lineage_prism_associations': ('27_build_lineage_prism_sparse.R;30_finalize_lineage_prism_server.R', 'PRISM AUC + effect/expression/CNV + Model', '56 个癌种×特征组合 complete、46 个样本不足；药物和基因两侧绝对相关 Top50 的并集。'),
'progeny_dependency': ('12_build_progeny_dependency.R', '表达 + Gene Effect', '14 条 PROGENy 通路 × 18,531 靶基因；1,117 共同模型；通路活性及相关性结果。'),
'wgcna_dependency': ('14_build_wgcna_dependency.R', '表达 + Gene Effect', '表达方差 Top5000；1,693 样本；11 个 WGCNA 模块，再关联 18,531 靶基因。'),
'lineage_gene_enrichment': ('31_build_lineage_gene_enrichment.R;34_finalize_lineage_gene_enrichment.R', '癌种内表达—依赖性排序 + Hallmark/Reactome/DoRothEA', '每源基因通路/TF 富集；20 个癌种 complete、14 个样本不足；与癌种选择性 fgsea 模块不同。'),
'cross_platform_validation': ('08_build_cross_platform_validation.R', 'Broad Gene Effect + Sanger CRISPR + DEMETER2 RNAi', '全局和癌种内共享基因的 Pearson/Spearman 一致性；31 个 parquet 结果。'),
'fixed_paper_cases': ('11_reproduce_fixed_cases.R', 'CNV + Gene Effect + Model', 'CCNE1 扩增、MYCN/DDX1 共扩增、Rhabdoid 三个固定例子；Rhabdoid 对比不等于直接 SMARCB1 基因型对比。'),
'lineage_selective_dependency': ('37_build_lineage_selective_dependency.R', 'Gene Effect + Model', '24 癌种分别对其余癌种的 limma 对比；每癌种 18,531 全基因表及 selective_hits。'),
'progeny_prism_associations': ('38_build_progeny_prism_associations.R', 'PROGENy 通路得分 + PRISM AUC', '14×1,482=20,748 检验，canonical FDR 显著 1,179；每通路 BH，需结合效应量和覆盖率审查。'),
'observational_synthetic_lethal_candidates': ('39_build_observational_synthetic_lethal.R', '既有 damaging/custom/hotspot/CNV 检验结果', '下游汇总，不重算 p 值；每事件 Top100，152,228 证据行、151,146 唯一配对。仅观察性候选。'),
'coamplification_dependency': ('40_run_coamplification_dependency.R;44_build_coamplification_pair_catalog.R;45_build_coamplification_dependency_screen.R;46_build_coamplification_lineage_adjusted.R', 'CNV + Gene Effect + Model；pair catalog 作为后续输入', '20,988,320 方向事件目录；15,368 对×18,531 靶点的发现层保留 13 hits；lineage-adjusted 层 15,267 可估计对、1,564 hits（1,187 对），两层须分开解释。'),
'bipolar_dependency_candidates': ('41_build_bipolar_dependency_candidates.R', 'effect_correlation 全量矩阵', '负相关 Top20 汇总：120,085 方向边、115,502 唯一配对、4,583 互惠 Top-K；旧 canonical 含低覆盖风险。'),
'lineage_selective_enrichment': ('42_build_lineage_selective_enrichment.R', 'lineage_selective_dependency 全基因排序 + Hallmark/Reactome', '24 癌种 fgsea 富集；enrichment 全表与显著子集。'),
'true_love_gene': ('47_build_true_love_genes.R;48_build_true_love_stability.R', 'effect_correlation；bootstrap 另读原始 Gene Effect', 'n≥500 的严格互为负相关 rank-1：510 对；canonical 稳定性与 audits 中双 seed 校准另存。'),
'subtype_dependency': ('49_audit_molecular_subtypes.R;50_build_subtype_dependency.R;51_verify_subtype_dependency.R', 'Model subtype/冻结规则 + Gene Effect', '同一癌种内 27 个 OncoTree + 6 个规则亚型对比，每个覆盖 18,531 靶点；33/33 QA PASS。'),
'lineage_wgcna_build_20260829': ('35_build_lineage_wgcna.R;36_verify_lineage_wgcna.R', '表达 + Model', '仅发现 9 个 ineligible_sample_n manifest，无可用分析结果；不可宣称癌种 WGCNA 已完成。'),
}

def script_links(names):
    result=[]
    for name in names.split(';'):
        p=local/name
        if p.exists(): result.append(f'[{name}]({p.as_posix()})')
        else: result.append(f'`/home/data/gz0548/depmap-agent/analysis/depmap-kb/{name}`')
    return '<br>'.join(result)

full = sorted((m for m in data['modules'] if m['module'].startswith('depmap-26q1-full/')),key=lambda m:-m['bytes'])
total=sum(m['bytes'] for m in full)/2**30
top=sum(m['bytes'] for m in full[:3])/2**30
lines=['# DepMap 数据概览审查 · 2026-09-05', '',
f'本次直接通过 SSH 读取 guotosky 的目录、文件元数据和 JSON manifest/QA，并核对本地与服务器生成脚本。证据采集时间：{data["captured_at"]}。', '',
f'**“180 多 G”主体是 depmap-26q1-full：文件逻辑大小 {total:.3f} GiB，约 {total*2**30/10**9:.2f} GB，30 个一级模块。** `du -h` 向上取整并按磁盘分配量显示为 183G。整个知识库目录 `du -sh` 为 197G，另有约 12G 数据工作区。', '',
f'最大的三个模块合计 {top:.2f} GiB，占主体 {top/total:.1%}。大量空间来自全量、未压缩 RDS 统计分块，包含非显著检验；并不代表同等数量的有效发现。', '',
'## 范围和证据等级', '',
'- 结果根目录：`guotosky:/home/data/gz0548/depmap-26q1`。以下主体表的模块路径均相对此目录的 `depmap-26q1-full/`。',
'- 早期构建脚本在 `D:/New-PHD/depmap_0823/analysis/depmap-kb`，manifest 保留本地生成路径；后续扩展脚本在服务器 `/home/data/gz0548/depmap-agent/analysis/depmap-kb`。本仓库 scripts 下有部分未编号版本。',
'- 原 TM00 示例脚本在 `D:/New-PHD/depmap_0823/tm00-script/scripts`。**TM00 编号与实际 builder 编号不是同一套编号**；例如 TM00 17 与 builder 17 含义不同。',
'- 本次完成目录/来源概览核对，没有重新跑 180G 分析、重算所有统计值或重新 hash 全部文件。脚本映射依据输入/输出代码与 manifest，不能替代带代码哈希的执行溯源。',
'- `complete` 和既有 QA PASS 是已有产物的声明；本次没有把它们升级为独立科学验证。旧模块有些 manifest 未写 status，需结合 block 覆盖与历史 catalog，不能自动推断失败或完成。', '',
'## 输入数据到分析的对应关系', '',
'| 数据源 | 主要用途 |', '|---|---|',
'| CRISPRGeneEffect.csv | 几乎全部依赖性分析的目标矩阵；共依赖、突变/CNV 背景、药物、通路、亚型分析 |',
'| CRISPRGeneDependency.csv、Gene.csv、Model.csv | 核心注释、依赖概率、癌种分组与覆盖资格 |',
'| OmicsExpressionTPMLogp1HumanProteinCodingGenes.csv | 共表达、表达—依赖、PROGENy、WGCNA、药物表达关联 |',
'| OmicsSomaticMutationsMatrixDamaging.csv | damaging 事件的泛癌/癌种内对比 |',
'| OmicsSomaticMutations.csv | custom_missense 多后果集合和 hotspot 事件对比 |',
'| OmicsCNGeneMC_WES.csv、ModelCondition.csv | 单扩增、共扩增、CNV—药物关联；阈值必须按模块区分 |',
'| PRISM_Repurposing_AUC_Matrix.csv、Compound Conditions | 药物 AUC 与依赖/表达/CNV/通路关联；AUC 越低表示越敏感 |',
'| Sanger、DEMETER2 | 跨平台一致性验证 |',
'| NextGen 2026 ScreenGeneEffect 等 | 3D/类器官分析，独立于 26Q1 2D 主体 |',
'| /refdir/database 下 TCGA 原始矩阵 | TCGA 表达及生存预计算桥接；不包含在本次 180G 主体内 |', '',
'服务器 `data/source` 约 8.7G、`data/nextgen_2026` 约 1.8G、`data/processed` 约 871M。本次未对整个 /refdir/database 做体积盘点。', '',
'## 主体：全部模块、脚本和结果', '',
'| 输出模块 | GiB / 文件数 | 实际构建脚本 | 输入 | 对应结果与审查语义 |', '|---|---:|---|---|---|']
for m in full:
    name=m['module'].split('/')[-1]
    builder,inputs,meaning=mapping[name]
    lines.append(f'| `{name}` | {m["bytes"]/2**30:.3f} / {m["file_count"]:,} | {script_links(builder)} | {inputs} | {meaning} |')

lines += ['', '## 主体之外的目录', '', '| 目录 | GiB | 用途 / 当前证据 |', '|---|---:|---|']
extra={
'depmap-26q1-core':'01_build_all_gene_core.R、02_audit_lineage_capabilities.R、03_build_lineage_dependency_tests.R；18,531 分析基因、44,083 注释基因、癌种描述/检验。',
'inputs':'prepare_depmap_3d_tm00_inputs.R 准备的 TM00 3D 输入。',
'tm00-script17-3d-run':'run_depmap_3d_tm00_true_love.sh 调用 3D codependency/true-love builder；独立未做 lineage adjustment 的对照运行。manifest 中 lineage_adjusted=false，虽子目录名仍带 lineage_adjusted。不能当作重复文件直接删除。',
'depmap-26q1-tcga':'build_depmap_tcga_bridge.R：33 个 TCGA 项目、18,531 靶基因、OS/DSS/DFI/PFI；既有 QA PASS。Cox score z 不是 HR。',
'depmap-26q1-tcga-pre-uppercase-backup':'基因符号大小写修正前 TCGA 备份，不应与现行结果混用。',
'audits':'全文件/脚本 inventory、bipolar 覆盖率网格、True Love 双 seed 稳定性、PROGENy 阈值敏感性；审计层不应混作 canonical。'}
for m in data['modules']:
    if m['module'] in extra: lines.append(f'| `{m["module"]}` | {m["bytes"]/2**30:.3f} | {extra[m["module"]]} |')

lines += ['', '### 3D 分析', '', '脚本位于服务器 `/home/data/gz0548/depmap-agent/analysis/depmap-3d`，本地版本位于 `D:/wisp_sci/scripts`。', '', '| 模块 | GiB | 构建脚本 | 当前结果 |', '|---|---:|---|---|']
three={
'audit':('audit_depmap_nextgen_3d.py','原始数据审计辅助文件'),
'codependency':('build_depmap_3d_codependency.R','3 cohort，共依赖相关矩阵；根 manifest complete'),
'dependency_profiles':('build_depmap_3d_dependency_profiles.R','5 组依赖性概览，complete'),
'differential_dependency':('build_depmap_3d_differential_dependency.R','4 个非配对差异对比，complete；可能保留平台差异'),
'lineage_dependency_enrichment':('build_depmap_3d_lineage_enrichment.R','10 组富集结果，complete'),
'omics_dependency':('build_depmap_3d_omics_dependency.R','4 模态，complete'),
'true_love_gene':('build_depmap_3d_true_love.R','2 cohort，complete；扩展审计记录 3512 / 3576 基础对，非稳定性最终结果'),
'extended_analysis_audit':('audit_depmap_3d_extended.R','108 模型、10 lineage；25 个合资格共扩增 pair；这是资格审计，不是共扩增检验完成证明'),
'true_love_stability':('build_depmap_3d_true_love_stability.R','仅 1 个 bootstrap_seed_2602.checkpoint.rds，无完成 manifest，不能标为完成'),
}
for m in data['modules']:
    if m['module'].startswith('depmap-26q1-3d/'):
        n=m['module'].split('/')[-1]; s,desc=three[n]
        lines.append(f'| `{n}` | {m["bytes"]/2**30:.3f} | [{s}](D:/wisp_sci/scripts/{s}) | {desc} |')
lines += ['', '**未在当前 3D 根目录发现 `coamplification_dependency` 和 `integrated_validation` 输出目录**；对应 builder 脚本存在，不等于执行完成。', '',
'## 优先审查事项', '',
'1. **候选版本：** bipolar canonical 仍为 115,502 唯一配对，与旧低覆盖版本一致。既有覆盖率审计 n≥500 为 3,556 对；不能直接把旧 canonical 用作正式发现集合。',
'2. **True Love：** 基础 510 对不等于稳健 510 对。既有双 seed、500-bootstrap 共同通过为 105 对，13 对属于跨阈值边界；审计输出与 canonical 分开保存。3D 稳定性目前只有 checkpoint。',
'3. **统计阈值：** 泛癌单扩增 >1.58 与共扩增 ≥2 是不同定义；custom_missense 包含 frameshift/stop/splice 等后果。审查时必须展开事件定义。',
'4. **稀疏存储：** lineage 网络、CNV、PRISM 只保留 Top-K，缺失行不能解释为生物学阴性。全量矩阵里的非显著结果也不能计为发现。',
'5. **多重检验：** 多数 FDR 是每 source、每药、每 pathway、每 pair 或每 contrast 内控制，不能写成全库全局 FDR。',
'6. **共扩增进度：** 旧本地审计说 lineage-adjusted 仍在运行，但当前服务器 manifest 已 complete，并有 QA PASS；15,267 可估计 pair、1,564 hits。应以本次快照覆盖旧进度描述，仍需比较调整前后变化。',
'7. **PROGENy—PRISM：** canonical 1,179 个 FDR hits；既有审计加入 |r|≥0.2 后为 185，n≥500 且 |r|≥0.2 后为 30。计数依赖规则，应并列展示而非单一“有效结果数”。',
'8. **溯源：** 历史 catalog 仍含 D:/ 路径及旧模块列表；部分 TCGA manifest 保留 building 路径；TM00 3D 子目录名与 lineage_adjusted=false 不一致。需要以现存路径、真实参数、输入版本核对。',
'9. **完成边界：** 癌种 WGCNA 仅有样本不足记录；3D 共扩增和整合验证没有输出，不能因为脚本存在就纳入已完成分析。', '',
'## 建议的审查顺序', '',
'先审三个占空间最大的模块：输入模型对齐、事件定义、每 block 源/靶覆盖、缺失值/有效 n、差值方向和 FDR family；再审稀疏模块的保留规则；最后冻结候选版本，单独列出未完成计算。', '',
'本次只新建概览报告和元数据证据文件，未修改服务器数据或重跑分析。详细证据见同目录 `server-evidence.json`，包括 45 个目录项的逐文件大小与采集到的 manifest/QA 内容。', '',
'历史方法审计参考：[全局分析审计](D:/wisp_sci/docs/depmap-tm00-global-analysis-audit.md)、[True Love 交叉验证](D:/wisp_sci/docs/true-love-tm00-cross-validation.md)。其中进度信息以本次服务器快照为准。', '',
'## 附录：各模块产物示例与 manifest 状态', '',
'下列状态计数按采集到的 JSON 文件计数，不代表分析/癌种数；父子 manifest 与 QA 可能同时计入。`unspecified` 表示没有 status 字段。', '']
for m in data['modules']:
    examples=[f['path'] for f in m['files'] if not f['path'].endswith('.json')][:4]
    states=Counter(v.get('status','unspecified') for v in m['manifests'].values())
    lines += [f'### {m["module"]}', '', f'实际路径：`{m["path"]}`。逻辑字节：{m["bytes"]:,}；文件数：{m["file_count"]:,}；manifest/QA 状态：`{dict(states)}`。', '', '产物示例：'+'；'.join(f'`{e}`' for e in examples), '']
assert len(full)==30 and set(mapping)=={m['module'].split('/')[-1] for m in full}
(OUT/'数据概览审查.md').write_text('\n'.join(lines),encoding='utf-8')
print(f'Report created: {len(full)} full modules, {len(data["modules"])} inventory entries; {total:.3f} GiB')
