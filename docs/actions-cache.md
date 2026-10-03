# Actions 缓存清理

目标是不超过 10 GB（10,000,000,000 bytes），不额外购买容量。只清理可重建的
重复旧编译快照，不删除 Actions artifacts、发行包、源码或游戏存档，不改 A 股
交易语义或构建参数。CI 保留为独立手动开发诊断，标签发布仅构建。

当前 workflow 生产者使用职责名 `distribution-native-cache` 与 `sealed-cargo-cache-no-debug`，
独立数值 `DISTRIBUTION_NATIVE_CACHE_FORMAT_VERSION=1` 与
`SEALED_CARGO_CACHE_FORMAT_VERSION=2` 拼入 cache key。恢复前缀与写入 key 使用同一
身份、版本、平台、架构、产品及 manifest hash 层级。普通依赖缓存使用
`rust-compile-only-cache` 与独立 `RUST_COMPILE_CACHE_FORMAT_VERSION=1`，不新增滚动清理。

`scripts/prune-actions-cache.mjs` 识别当前 native 格式 1 与 sealed 格式 2，并明确读取历史
`distribution-native-v1`、`sealed-cargo-v2-no-debug` 系列。按 `(cache.ref, matched series prefix)`
分别保留最新一份；历史系列与新系列各自最新项都受保护，不能因命名重构退休另一系列。
即使 manifest hash 改变，同一系列仍只保留最新快照。pnpm、普通 Cargo 和未知格式或平台
缓存不删除，失败构建的最新编译进度也保留。

需 `gh` 已登录，并有仓库 Actions 缓存删除权限。默认仅预览，显式 `--apply`
才执行删除；整个清理使用进程外 300000ms deadline，单次网络命令限时 30000ms。

```sh
node scripts/run-long-validation.mjs 300000 -- node scripts/prune-actions-cache.mjs --repo msuadOf/stock_market_game
node scripts/run-long-validation.mjs 300000 -- node scripts/prune-actions-cache.mjs --repo msuadOf/stock_market_game --apply
```

每次删除前重新查询快照，避免更新缓存已经消失时删除最后一份。发现其他 workflow
运行、排队或等待时明确延期；不会将延期冒充容量已达标。在构建任务全部完成后的
收尾 job 中调用时，`GITHUB_RUN_ID` 只用于排除当前收尾所在的 run。

## 自动清理

手动 `ci.yml` 和 `distributions.yml` 接入独立的 `prune-caches` 收尾 job，分别等待
`build` 和 `frontend/native/server` 的全部任务结束（包含 cache action 的 post
步骤），构建失败也可清理；workflow 被取消或由 PR 触发时不执行清理。
标签发布由 `release.yml` 收尾统一清理，等待分发、Release 发布和 Pages 部署结束；
成功公开 Release 后退休本标签缓存，失败保留最新编译进度。分发 workflow 的清理
job 在标签 run 中跳过，避免提前退休缓存。三个清理 job 共用仓库级
`actions-cache-prune` concurrency group，不互相取消。
只有清理 job 获得 `actions: write`，构建步骤没有新增删除权限，PR 不获得删除权限。

产品构建和标签发布的清理直接在 300000ms 进程树 deadline 内调用 `--apply`，
保留活动 run 检查、删除前重读计划和清理后容量核验，不运行缓存契约测试。
手动 CI 的清理仍先在 10000ms 进程树 deadline 内执行缓存策略和 workflow 契约
短测，保留默认文件进程隔离，并发上限 4；再调用受限清理命令。
有其他活动 workflow 时明确延期，由之后的收尾任务再次尝试；若最后结束的是 PR
或取消的 workflow，可能需要等待下一次非 PR 构建或手动执行清理命令。
不安装 Rust/pnpm，不重新编译或额外执行完整回归。

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
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 scripts/prune-actions-cache.test.mjs scripts/cache-workflow.test.mjs
```
