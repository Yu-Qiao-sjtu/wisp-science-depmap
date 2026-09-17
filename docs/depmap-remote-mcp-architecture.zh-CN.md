# DepMap Agent 远程数据桥接技术路线

本文描述 Wisp Science 如何把用户自然语言问题映射到服务器上的 DepMap 26Q1 预计算结果，并将结构化证据交给模型解释。目录索引用于路由和定位；科学问题的最终回答必须以实际结果为主，路径与模块状态只用于溯源。

## 完整链路

```mermaid
flowchart TD
    U["用户自然语言问题<br/>例如：TP53突变后依赖哪些基因？"]
    U --> A["Wisp Science<br/>DepMap Agent"]
    A --> B{"用户意图识别"}

    B -->|分析目录问题| I1["analysis_inventory"]
    B -->|固定癌种选突变基因| I2["mutation_anchor_discovery"]
    B -->|固定突变找依赖| I3["mutation_to_dependency"]
    B -->|固定依赖找突变| I4["dependency_to_mutation"]
    B -->|基因对相关性| I5["gene_pair_evidence"]
    B -->|表达预测依赖| I6["expression_biomarker_model"]
    B -->|TF活性与依赖| I7["tf_activity_to_dependency"]
    B -->|方向不明确| C["澄清卡片<br/>固定突变还是固定依赖？"]
    C --> U

    I1 --> R["Agent路由器<br/>depmap_agent_route"]
    I2 --> R
    I3 --> R
    I4 --> R
    I5 --> R
    I6 --> R
    I7 --> R

    R --> D{"选择受限查询能力"}
    D -->|目录盘点| T1["depmap_analysis_catalog"]
    D -->|癌种突变锚点| T2["突变锚点查询工具"]
    D -->|突变到依赖| T3["突变依赖查询工具"]
    D -->|依赖到突变| T4["反向突变查询工具"]
    D -->|基因对| T5["depmap_pair_evidence"]
    D -->|预测模型| T6["depmap_biomarker_model_evidence"]
    D -->|TF活性| T7["depmap_tf_dependency_evidence"]

    T1 --> M["Wisp MCP客户端"]
    T2 --> M
    T3 --> M
    T4 --> M
    T5 --> M
    T6 --> M
    T7 --> M

    M --> L["本地回环入口<br/>127.0.0.1:18877/mcp"]
    L --> S["SSH加密隧道"]
    S --> RM["服务器DepMap MCP<br/>127.0.0.1:8877"]
    RM --> Q{"查询策略"}

    Q -->|确定结果位置| CAT["SQLite目录索引"]
    Q -->|高频稀疏结果| IDX["SQLite内容索引"]
    Q -->|大型矩阵或分块| FILE["RDS / Parquet / CSV.GZ<br/>按索引读取指定结果"]

    CAT --> AC["analysis_catalog<br/>分析单元与完成状态"]
    CAT --> AR["artifact_catalog<br/>文件格式与相对位置"]
    CAT --> CC["capability_catalog<br/>意图与查询能力"]
    IDX --> TL["true_love"]
    IDX --> TF["tf_dependency"]
    IDX --> BM["biomarker_target"]

    AC --> E["受限结果提取"]
    AR --> E
    CC --> E
    TL --> E
    TF --> E
    BM --> E
    FILE --> E

    E --> P["结构化证据包"]
    P --> P1["结果：基因、癌种、排名"]
    P --> P2["统计：效应量、样本数、P值、FDR"]
    P --> P3["语义：指标定义与效应方向"]
    P --> P4["状态：FOUND / NOT_RETAINED等"]
    P --> P5["溯源：版本、manifest、depmap URI"]

    P1 --> X["Wisp Science模型解释层"]
    P2 --> X
    P3 --> X
    P4 --> X
    P5 --> X
    X --> V{"结果类型"}

    V -->|科学结果| O1["结果优先回答<br/>结论→结果表→解释→限制→来源"]
    V -->|目录问题| O2["目录回答<br/>完成模块→状态→可查询能力"]
    V -->|没有预计算结果| O3["说明覆盖缺口<br/>不把路径冒充结果"]
    V -->|需要新计算| O4["提出按需分析方案<br/>授权后建立Run"]

    O1 --> F["最终呈现给用户"]
    O2 --> F
    O3 --> F
    O4 --> F
```

## 核心数据流

```mermaid
flowchart LR
    A["用户问题"] --> B["意图与实体"]
    B --> C["目录索引"]
    C --> D["远程数据定位"]
    D --> E["结果读取"]
    E --> F["结构化证据"]
    F --> G["模型科学解释"]
    G --> H["结果优先回答"]
```

## 各层职责

| 层级 | 主要职责 | 禁止行为 |
|---|---|---|
| 用户意图层 | 判断用户要固定癌种、突变基因还是依赖靶点 | 猜测缺失的关键方向 |
| 路由层 | 选择 MCP 工具并校验必要参数 | 把路由记录当作科学证据 |
| 目录索引层 | 确定结果位置、版本、完成状态和查询入口 | 把路径当作最终科学结果 |
| 数据查询层 | 读取有限结果行或指定矩阵分块 | 遍历或载入整个知识库 |
| 结构化传输层 | 传递统计量、语义、状态与来源 | 只传文件名或模块名称 |
| 模型解释层 | 解释效应方向、显著性、生物学含义和限制 | 将相关性描述为因果关系 |
| 用户回答层 | 展示结论和结果表，最后附来源 | 要求用户自行打开服务器文件获取已有结果 |

## 索引与数据边界

统一 SQLite 索引分为两类：

- **目录索引**：`analysis_catalog`、`artifact_catalog` 和 `capability_catalog`，负责定位模块、分析单元、相对路径、状态及查询能力；
- **内容索引**：为高频稀疏结果提供直接检索，例如 `true_love`、`tf_dependency` 和 `biomarker_target`。

大型相关矩阵继续保留为 RDS、Parquet 或压缩表格。查询根据目录索引定位所需结果或分块，不将完整矩阵复制到 SQLite，也不把整个知识库送入模型上下文。

服务器 MCP 只监听服务器回环地址。Wisp Science 使用本机回环端口，通过 SSH 加密隧道访问服务器 MCP；远程端口不直接暴露到公网。返回的 `depmap://26Q1/...` 是知识库内的稳定溯源标识，不是要求用户自行访问的文件路径。

## 结果传输契约

除明确的目录盘点问题外，MCP 返回包使用 `scientific_result` 契约，至少包含当前查询可获得的以下内容：

1. 查询实体和规范化癌种；
2. 有限数量的结果行与排名；
3. 效应方向、效应量和指标定义；
4. 样本量、P 值和 FDR（分析产生这些字段时）；
5. `FOUND`、`NOT_RETAINED`、`INELIGIBLE`、`NOT_COMPUTED` 或 `MODULE_UNAVAILABLE` 状态；
6. DepMap 发布版本和结果溯源。

`presentation_contract` 强制解释模型遵守以下规则：

- 科学问题以实际结果为主要内容；
- 不用目录状态或路径替代结果；
- 按返回指标解释方向，不更改统计量含义；
- 区分观测关联、候选机制和因果结论；
- 将版本与 provenance 放在回答末尾。

## 标准用户回答顺序

1. 直接结论；
2. 关键结果表；
3. 效应方向与统计证据；
4. 生物学解释；
5. 分析限制和下一步；
6. DepMap 版本及结果来源。

如果目录中存在分析模块，但当前接口没有返回可解释的结果行，Agent 必须说明当前覆盖或查询接口的缺口，不得把模块名称、完成状态或文件位置包装成科学结论。
