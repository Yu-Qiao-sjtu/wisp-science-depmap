# ESR1 按需 Hallmark GSEA 测试记录

## 验收环境

- 日期：2026-09-08
- DepMap 发布版：26Q1
- 服务器模块：`/home/data/gz0548/depmap-26q1/analysis-modules/表达基因-CRISPR基因依赖相关性分析`
- 表达源基因：ESR1
- 范围：全局 1,140 个表达—Gene Effect 公共细胞系
- 基因集：MSigDB 2026.1.Hs Hallmark
- 排序：`negative_signed_t`
- 完整配对门槛：`pair_n >= 912`（1,140 的 80%）
- 实际进入 GSEA：17,787 个依赖基因

## 执行命令

```bash
Rscript scripts/run_expression_dependency_gsea.R \
  --matrix-root results/expression_dependency \
  --source-gene ESR1 \
  --collection hallmark \
  --cache-root results/expression_dependency_gsea_cache
```

## 结果验收

- 完成 50 个 Hallmark 通路检验；2 个通路达到源基因内 `FDR <= 0.05`。
- `HALLMARK_APICAL_SURFACE`：NES 约 2.05，FDR 约 0.0012。正 NES 表示 ESR1 表达越高，通路成员总体越倾向于更强的 CRISPR 依赖。
- `HALLMARK_MYC_TARGETS_V2`：NES 约 -2.12，FDR 约 0.0009。负 NES 表示 ESR1 表达越高，通路成员总体越倾向于更弱的 CRISPR 依赖。
- `qc.json` 状态为 `pass`，发布版为 26Q1。
- 再次运行相同命令返回 `cache.hit=true`，没有重复执行富集。

结果目录：

```text
results/expression_dependency_gsea_cache/26Q1/global/hallmark/
  MSigDB-2026.1.Hs/01925_ESR1/
  negative_signed_t__minpair-0912__size-15-500__top-20__seed-20260908/
```

该目录包含：

| 文件 | 大小（本次验收） | 用途 |
|---|---:|---|
| `enrichment.parquet` | 16,566 bytes | 50 个通路的完整统计结果 |
| `ranked_dependency_targets.parquet` | 450,272 bytes | 17,787 个依赖基因的完整排序 |
| `top_pathways.pdf` | 8,152 bytes | 主要正、负 NES 通路图 |
| `result.json` | 12,670 bytes | Agent 使用的紧凑结果 |
| `run_manifest.json` | 3,198 bytes | 输入、参数、软件与数据库版本 |
| `qc.json` | 864 bytes | 质量检查 |

文件大小会因软件序列化和数值精度发生少量变化，应以结构、记录数、参数和 QC 为验收依据。

## 解释限制

这是表达量与 CRISPR Gene Effect 相关排序上的观察性富集。它用于提出通路层面的依赖假设，不能单独证明 ESR1 直接调控这些基因、产生合成致死或预测药物疗效。
