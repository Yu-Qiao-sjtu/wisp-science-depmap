# DepMap 分析模块

本目录按科学问题组织 DepMap 分析。每个模块同时说明“分析什么、使用什么数据、由哪些脚本产生、结果放在哪里、agent 应如何识别用户意图”，避免脚本、结果和解释继续散落在仓库与服务器的不同位置。

## 当前模块

| 模块 | 数据模态 | 关系 | 当前状态 | 服务器结果规模 |
|---|---|---|---|---:|
| [表达基因—表达基因共表达分析](./表达基因-表达基因共表达分析/README.md) | `log2(TPM + 1)` 表达量 | 共表达 | 全局矩阵与癌种内网络已完成 | 约 6.51 GiB |
| [CRISPR 基因—基因共依赖分析](./CRISPR基因-基因共依赖分析/README.md) | CRISPR Gene Effect | 共依赖 | 全局矩阵与癌种内网络已完成 | 约 7.43 GiB |
| [表达基因—CRISPR 基因依赖相关性分析](./表达基因-CRISPR基因依赖相关性分析/README.md) | `log2(TPM + 1)` × CRISPR Gene Effect | 表达—依赖关联 | 全局矩阵与癌种内网络已完成 | 约 6.27 GiB |

## 标准目录

每个分析模块使用以下结构：

```text
analysis-modules/<模块名称>/
├── README.md                 # 目的、范围、完成状态
├── 分析方法与审查说明.md      # 统计逻辑、口径、限制
├── module.intent.json        # agent 意图路由标签
├── data/
│   └── 输入数据清单.md        # 数据来源、结构、校验值、服务器位置
├── scripts/
│   ├── <可执行脚本>
│   └── 脚本来源.md            # 原脚本位置与快照哈希
└── results/
    └── 结果数据索引.md        # 产物、格式、规模、服务器位置
```

## Git 与服务器边界

- Git 保存脚本、方法说明、输入清单、结果索引、意图标签和小型测试记录。
- CSV、RDS、Parquet、矩阵分块等实体数据保存在服务器 `/home/data/gz0548/depmap-26q1/analysis-modules/`。
- 模块文档必须记录服务器路径、样本/基因口径、文件格式和校验信息。不能只写“数据在服务器”。
- `knowledge/depmap-26q1-full/` 下保留兼容软链接时，以模块目录中的实体文件为准。

## 分支与工作树规则

1. 每个新模块从最新 `origin/main` 建立独立分支和工作树，分支名使用 `codex/analysis-YYYYMMDD-<module>`。
2. 一个分支只整理一个分析问题；共享路由或 schema 的改动应与该模块一起审查，或单独开基础设施 PR。
3. 模块完成后先运行相关验证，再提交 PR；合并后再从更新后的 `origin/main` 开始依赖它的下一个模块。
4. 独立模块可以并行，存在脚本或 schema 依赖的模块按合并顺序串行。
5. 不在长期脏工作树上继续叠加新模块，也不从尚未合并的功能分支派生无关模块。

## Agent 意图标签

`module.intent.json` 至少要明确以下字段：

- `module_id`：稳定模块标识。
- `analysis_label`：分析任务标签。
- `data_modality`：数据模态，例如表达量或 CRISPR Gene Effect。
- `relation_type`：关系类型，例如 `coexpression` 或 `codependency`。
- `scope`：全局或癌种内。
- `cohort_policy`：样本集合的选择规则。
- `metric`：相关系数、效应量等核心统计量。

用户只说“两个基因是否相关”时，agent 不应静默选择数据模态；应分别返回共表达与共依赖结果并标明样本口径。
