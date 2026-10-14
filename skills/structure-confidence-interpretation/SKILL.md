---
name: structure-confidence-interpretation
description: "Use this skill AFTER any protein/nucleic-acid/ligand structure prediction has finished (AlphaFold Server / AF3, Boltz-2, OpenFold3, Chai-1, AlphaFold2, ESMFold), whenever the user asks what the prediction result means, what the colors on the structure mean, how to read pLDDT / PAE / PDE / pTM / ipTM / confidence_score, whether a predicted complex interface is trustworthy, or asks for 结果解读, 置信度解释, 颜色含义, PAE 图怎么看, pLDDT 是什么. Also use proactively whenever you deliver a predicted structure to the user: every delivered prediction MUST be accompanied by a confidence interpretation section built from this skill."
---

# Structure Confidence Interpretation — 预测结构结果的置信度解读

Structure prediction models (AF3-style co-folders) output not only coordinates but
a self-assessment of how trustworthy those coordinates are. Delivering a predicted
structure WITHOUT explaining these metrics is the single most common way users get
misled — a beautiful cartoon can hide a fabricated domain orientation.

This skill is the mandatory interpretation layer. Default language: Chinese, key
technical terms kept in English. Use another language only if the user asks.

## Hard constraints

1. **Never deliver a predicted structure without an interpretation section.** Every
   prediction summary must include: the metric table (below), which regions/blocks
   are trustworthy, which are not, and one explicit "能不能信" verdict.
2. **Read the actual numbers from the output files** (`*_confidences*.json`,
   `confidence_*.json`, `summary_confidences_*.json`, `scores_confidences_*.json`,
   or the AlphaFold Server report). Never invent pLDDT/ipTM values.
3. **Interpret PAE as blocks, not as a single number.** Domain-internal and
   chain-pair blocks carry different meanings; the conclusion for a complex is
   driven by the inter-chain blocks, not the global mean.
4. **Distinguish "model is unsure" from "biology is disordered".** Low pLDDT on a
   linker is usually a real flexible/disordered region, not a failed prediction.
5. **For multi-state proteins (e.g., multi-domain receptors), state explicitly that
   inter-domain orientation is one sampled conformation** unless the inter-domain
   PAE block is strong (< ~5 Å).

## The metric table (always include, adapted to the run)

| 指标 | 回答的问题 | 判读阈值 |
|---|---|---|
| pLDDT (0–100) | 这个残基自己的局部结构画对了吗 | >90 高置信；70–90 主干对、侧链别抠；50–70 存疑；<50 视为无序/噪声 |
| PAE (Å, 残基对矩阵) | 残基 A 与 B 的相对位置对吗（域间/链间取向） | <5 Å 可信；5–15 Å 存疑；>15 Å 该块取向是模型猜测 |
| PDE / gpde (原子/侧链取向) | 侧链与配体间距离、朝向对吗 | 分析结合位点、氢键、盐桥时参考；gpde 越接近 1 越好 |
| pTM (0–1) | 整体折叠拓扑对吗 | >0.5 及格；单链折叠看这个 |
| ipTM (0–1) | 链间界面质量（复合物核心指标） | >0.8 界面可信；0.6–0.8 换 seed 重跑验证；<0.6 复合物预测不可信 |
| confidence_score (Boltz/Chai 排序分) | 多个 sample 里选哪个 | 用于排序 sample，不等价于 ipTM，不要单独当"对错"用 |
| has_clash (0/1) | 是否有原子碰撞 | =1 的 sample 直接淘汰 |
| affinity_probability_binary (Boltz 亲和头) | 小分子是不是 binder | >0.5 提示结合，仅排序用，不是 KD |

## Structure colors (what the rainbow means)

- **AlphaFold Server / AF2 / AFDB / ESMFold 按链着色的 pLDDT 模式**：蓝(100)→青→黄→橙(50)→红(<50)。
  - **蓝色**：backbone 可当解析结构使用；
  - **青色**：总体可信，细节正常看待；
  - **黄/橙色**：可能对可能错，讨论构象时须谨慎措辞；
  - **红色**：模型无约束，通常是内在无序区（IDR）——直接告诉用户"这里本来就没有固定结构"。
- **PAE 矩阵热图**：对角线上的方块 = 各链/结构域内部（通常低=好）；非对角块 =
  域间/链间相对取向（低=界面或连接可信；发黄发红=取向不可信）。
- 在 ChimeraX 里复现着色：`color byattribute bfactor`（把 pLDDT 写入 B-factor 后），
  或直接告诉用户下载的 mmCIF 已带 B-factor 列。

## Reading the numbers from each tool's output

```bash
# Boltz-2: out/boltz_results_*/predictions/*/confidence_*.json
#   keys: iptm, ptm, complex_plddt, confidence_score, (affinity_*.json)
# OpenFold3: <query>/seed_*/..._confidences_aggregated.json
#   keys: avg_plddt, ptm, iptm, gpde, has_clash, sample_ranking_score
# Chai-1: scores.model_*.npz  (iptm, ptm, aggregate_score)
# AlphaFold2: ranked_*.json (iptm/ptm) + predicted_aligned_error *.json
# AlphaFold Server: 网页展示 + 下载 zip 内含 pae/pde 矩阵与评分 json
```

以 OpenFold3 为例，先读小文件 `*_confidences_aggregated.json`：

```json
{"avg_plddt": 78.96, "ptm": 0.667, "iptm": 0.0, "gpde": 0.73,
 "has_clash": 0.0, "sample_ranking_score": 0.133}
```

单链输入时 `iptm` 可能为 0 或缺失——ipTM 只对多链有意义，此时看 `ptm`。
同理 Boltz 单链输入读 `ptm` 与 `complex_plddt`，别读 `iptm`。

## Verdict wording (输出模板)

预测交付时按此结构给出结论（数值替换为实读值）：

> **结果解读**
> - 整体折叠：pTM = 0.86（>0.5，拓扑可信）。
> - 局部结构：pLDDT 中位数 92；N 端 1–34 与 linker 512–560 为橙色/红色，属柔性/无序区，不代表预测失败。
> - 界面/域间：ipTM = 0.83，链 A–B 的 PAE 交叉块中位 ~3.5 Å（<5 Å），结合模式可信。
> - 结论：结构域内部与界面可以采信；全长链的域间相对取向（PAE 块 8–12 Å）仅为模型采样的一个构象，下生物学结论前需换 seed/多 sample 或实验验证。

## Plotting the PAE heatmap (helper script)

Skill 目录内附带 `scripts/plot_confidence.py`：读 Boltz/OpenFold3 的 confidence
JSON 或 AlphaFold Server 的 PAE 矩阵，输出 PAE 热图 PNG 并打印指标摘要。用 Wisp
的 Python 内核或 `python skills/structure-confidence-interpretation/scripts/plot_confidence.py <files...>` 运行。

## Common failure modes to flag proactively

- ipTM 高但配体是离子/小分子且接触面极小 → "置信度高"≠"有生物学意义的结合"。
- 用户只给了一段序列（无 MSA 服务）→ 提醒单序列模式精度整体下降一档。
- 多 sample 的 PAE 图差异巨大 → 该体系存在构象异质性，报告时应并列展示而不是只给排名最高的。
- 预测了突变体/翻译后修饰 → 模型对这类输入是外推，pLDDT/PAE 偏乐观，结论措辞要更保守。
