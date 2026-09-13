# guotosky 服务暴露修复记录

日期：2026-08-31

## 问题

主要暴露来自 Linux 进程命令行。普通 `ps` 输出直接显示了分析目录、脚本名和参数，例如分析主题、真实数据根目录和知识库目录。`SIGSTOP` 只能暂停计算，不能隐藏 `/proc` 中已有的命令行，因此暂停状态仍然会泄露研究内容。

排查时还发现旧 DepMap Explorer 服务通过 `0.0.0.0:8780` 监听全部网络接口。服务由 crontab 中每 5 分钟运行一次的 watchdog 自动恢复，单独终止进程不能持续阻止暴露。

服务源码中另有服务器账号相关的硬编码绝对路径：

`/home/data/gz0548/depmap-agent/data/processed`

这会把部署位置与账号目录固化到代码中，并增加路径进入日志、异常或返回内容的风险。

## 已采取的措施

1. 停止公开监听 `0.0.0.0:8780` 的旧 DepMap Explorer 服务。
2. 停止仅供 SSH 隧道使用的 `depmap-kb-api.service`，修复期间不保留项目服务监听。
3. 先使用 `SIGSTOP` 暂停计算链；确认暂停仍会暴露命令行后，终止全部相关计算与后续等待链。原任务需要从现有输出或检查点重新启动，不能对已终止 PID 使用 `SIGCONT`。
4. 为 watchdog 增加 `logs/server.paused` 暂停标记；标记存在时不会重启服务。
5. 将旧服务默认监听地址改为 `127.0.0.1`，并通过 `DEPMAP_HOST` 显式配置；拒绝依赖 `0.0.0.0` 默认值。
6. 删除业务代码里的账号绝对路径，统一使用 `DEPMAP_DATA_DIR` / `data_loader.DATA_DIR`。
7. 删除文档中的“局域网访问”指引，明确服务只允许本机回环访问，远程使用必须通过 SSH 本地端口转发。
8. 将远端 `analysis`、`logs`、`data` 和知识库目录权限收紧为 `700`，并移除其中普通文件的 group/other 权限。
9. 新增通用私有 R runner。真实脚本路径和参数只写入权限为 `600` 的任务规格文件，并通过环境变量交给 runner；`ps` 只显示中性的 `~/.wisp-private-run/runner.R`。

## 当前状态

- `8780`：无监听。
- `8876`：无监听。
- 原计算链：已终止，防止暂停进程继续暴露命令行。
- 修复后的计算：已通过私有 runner 并行启动 `j01` 和 `j02`；未启动 3D 分支。
- 进程扫描：当前没有包含 `analysis/`、具体构建脚本名、真实数据路径或知识库路径的相关进程。
- watchdog：由 `logs/server.paused` 阻止自动拉起。
- 私有目录权限：`analysis`、`logs`、`data` 和知识库均为 `700`。

## 计算恢复与断点

任务规格、日志和状态分别位于仅本人可访问的 `~/.wisp-private-run/specs/`、`logs/` 和 `state/`。当前并行任务为：

1. `j01`：co-amplification lineage-adjusted；
2. `j02`：基于已完成 2D `effect_correlation` 共依赖矩阵的 True Love stability。

2D 共依赖矩阵和基础 True Love Gene manifest 已经完成。此前准备的 3D codependency、3D True Love、3D omics 和 3D validation 规格没有启动，不属于本次并行恢复范围。

断点分为两层：

- 完成标记：任务成功结束后可创建 `state/jNN.done`。
- 任务级断点：`j01` 按 shard 保存并跳过完整 shard；`j02` 每 5 次 bootstrap 原子保存累计计数、相关矩阵与 RNG 状态。

链状态通过 `state/current` 和 `state/chain.pid` 查询。日志使用无研究语义的编号文件名，不再把分析名称放到进程命令行。

## 安全恢复步骤

不要直接用 `Rscript /真实/分析脚本.R --data-root=...` 恢复任务，这会再次通过进程列表暴露路径。为任务创建私有规格文件：第一行是真实脚本路径，其余每行一个参数；规格目录必须是 `700`，文件必须是 `600`。然后只用不含研究语义的任务编号启动：

```bash
WISP_PRIVATE_RUN_ROOT="$HOME/.wisp-private-run" \
  "$HOME/.wisp-private-run/run-job" j01
```

启动后必须使用 `ps -f` 验证：命令行只能出现中性 runner 路径，不得出现真实脚本、数据或知识库路径。

恢复旧 Explorer 服务前，应确认 `DEPMAP_HOST=127.0.0.1`，然后删除暂停标记并手工运行 watchdog：

```bash
rm /home/data/gz0548/depmap-agent/logs/server.paused
DEPMAP_HOST=127.0.0.1 /home/data/gz0548/depmap-agent/app/server-ensure.sh
```

如需恢复知识 API：

```bash
systemctl --user start depmap-kb-api.service
```

知识 API 保持 `127.0.0.1:8876`，客户端应继续通过 SSH 本地端口转发访问，不应改成公网监听。

## 验证命令

```bash
ss -lntp | grep -E ':(8780|8876)\\b' || true
ps -o pid,ppid,stat,etime,args -p 1936844,1977630,2089761
ps -u "$USER" -o pid,ppid,stat,etime,args | \
  grep -E 'analysis/|depmap-26q1|nextgen_2026|build_.*\\.(R|py)' || true
```
