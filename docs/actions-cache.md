# Actions 缓存清理

目标是不超过 10 GB（10,000,000,000 bytes），不额外购买容量。只清理可重建的
重复旧编译快照，不删除 Actions artifacts、发行包、源码或游戏存档，不改 A 股
交易语义、构建参数或回归门禁。

`scripts/prune-actions-cache.mjs` 识别现有 `distribution-native-v1` 和
`sealed-cargo-v2-no-debug` 滚动 key，按分支 ref、平台、架构、产品各保留最新
一份。即使 manifest hash 改变，同一系列仍只保留最新快照。pnpm、普通 Cargo
和未知格式缓存不删除，失败构建的最新编译进度也保留。

需 `gh` 已登录，并有仓库 Actions 缓存删除权限。默认仅预览，显式 `--apply`
才执行删除；整个清理使用进程外 300000ms deadline，单次网络命令限时 30000ms。

```sh
node scripts/run-long-validation.mjs 300000 -- node scripts/prune-actions-cache.mjs --repo msuadOf/stock_market_game
node scripts/run-long-validation.mjs 300000 -- node scripts/prune-actions-cache.mjs --repo msuadOf/stock_market_game --apply
```

每次删除前重新查询快照，避免更新缓存已经消失时删除最后一份。发现其他 workflow
运行、排队或等待时明确延期；不会将延期冒充容量已达标。在构建任务全部完成后的
收尾 job 中调用时，`GITHUB_RUN_ID` 只用于排除当前收尾所在的 run。本工具目前
未接入 workflow，需与同时进行的构建脚本修改协调后再接入。

API/删除失败明确报错，清理后再次查询实际缓存容量。若只剩保留项仍超标，明确
失败并报告，不擅自牺牲最新可用缓存。GitHub API 并非事务，构建中或并发新建
缓存时可能暂时超标，不能承诺任意时刻严格低于上限。多实例清理应共用 Actions
concurrency group；不能授予来自 PR 的任意代码缓存删除权限。

## 本轮实际清理

2026-10-02 已通过 `gh` 删除 7 条被更新快照替代的缓存（6 条原生分发、1 条
Windows 密封编译缓存）。列表总量从 22 条、10,457,765,896 bytes 降至 15 条、
6,180,046,550 bytes（约 6.18 GB / 5.76 GiB），释放 4,277,719,346 bytes。
保留三个平台、各产品最新可用编译缓存及全部 pnpm/普通 Cargo 缓存。仓库 usage
汇总接口可能滞后于缓存列表，不能把尚未同步的汇总当成删除失败或重复盲删。

代表性短测：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 --test-isolation=none scripts/prune-actions-cache.test.mjs
```
