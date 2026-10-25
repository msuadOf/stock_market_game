# #125：公共价格笼子边界收尾

本任务仅对应 `price-cage-floor` issue #125，与 B4d 日终重集成任务无关。
基线为 `f58202911b8db6c042fe1816d08277c16381f2d2`，独占工作区 `.worktree/finish-125-price-cage`。
旧 `.worktree/day-end-public-views` 仅作为只读历史来源，未修改其 dirty 文件。

## 核对结论与最小范围

基线已含 18 个 `day_end_public_views_tests` 用例，生产代码尚未包含 #125 修复。
旧树多含过期 #121 代码，不能整体复制。仅移植 `desk` 公共边界计算、错误传播与相位 helper，
`world::initial_views` 按首个 Start 相位应用笼子，再补首拍五相位/开关用例。

旧 dirty 树额外四例中的两项失败 `initial_public_view_matches_the_first_start_phase`、
`first_start_accepts_exactly_the_initial_public_cage_bounds` 要求首拍买向上限 1122 分，
原实现却报日涨停 1210 分；真实 `desk` 会拒绝 1123 分。此为同一观察/受理边界根因，
Leader 已同意最小补齐 `initial_views.max_buy_price` 对称分支。旧树上述两项正确目标的断言完整移植；基线两项“记录已知错误”的测试则改为正确卖笼子目标、加强真实受理边界，并用原生产代码跑出红证，未改撮合与受理算法。
另外补低价十档更宽及非整分百分比四舍五入两个 fixture。

## 官方规则与适用日期

2026-10-25 实际联网读取，适用 2026-07-06 起施行的规则：

- [上交所《交易规则（2026 年修订）》官方原件](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/10816482/files/959da0158c65434daa8a43a6e32be7ba.docx)，3.3.14：连续竞价限价申报买向 102% 与以上十个最小价位孰高、卖向 98% 与以下十个最小价位孰低；无盘口时最新价/前收。3.3.17：四舍五入，不足一档至少移动一档。
- [深交所《交易规则（2026 年修订）》官方原件](https://docs.static.szse.cn/www/lawrules/rule/trade/current/W020260424690713155663.pdf)，3.3.16 与 3.3.19 对应相同边界及取整要求。

旧台账称“98% 向下取分”不准确：当前 `desk::bound_price` 使用四舍五入，`upward` 仅指定最小移动方向；本批沿用正确算法，不新增“取整简化”。
本批 1100 分参考价时卖下限 1078、买上限 1122；200 分时十档更宽，卖 190/买 210；1026 分时四舍五入为卖 1005/买 1047。
公共视图只承诺观察时的边界，后续盘口变动后实际受理仍由 `desk` 独立校验；关闭笼子仍属已登记游戏简化。

## 验证记录

证据目录：主工作区 `.tmp/branch-completion/issue125/`。构建目录：主工作区 `.tmp/build-cache/finish125/`。
构建为独立长构建（不是完整回归），`-j 128`，进程外共享 300000ms deadline；普通测试每命令进程外 10000ms，`--test-threads=32`。
未启动全量回归，由 Leader 统一执行。未推送、未合并 main；完整 diff 待未实施 Sol 独立复核。

### 本轮实际结果与证据绑定

- 基线含 18 个用例，其中 #121 部分 Start 发送/回收等五例保留；旧 dirty 额外四例完整移植；新增首拍卖向五例、低价/取整一例（内部两个 fixture）、构造失败一例。最终相关套件共 29 个 case。
- 有效首轮红：`red.stdout` / `red.stderr`，28 中 21 过、7 失败；`red-result.json` 记录 supervisor exit 1、0.248s；生产代码仍为 f582，测试源 hash 见 `red-source-hashes.json`、完整测试见 `red-tests.rs`。错误包括 990 对 1078、1210 对 1122、200 分参考价的 180/220 对 190/210。
- 构造失败红：`error-red.stdout` / `error-red.stderr`，新增一例 0/1；worker 构造调用次数实际 1、期望 0，0.184s。`initial_views` 的 Result 引入可失败路径，必须在启动 worker 前计算，否则返回错误会丢弃 JoinHandle。修复仅前移计算，原件见 `error-red-world.rs` / `error-red-tests.rs`，绑定见 `error-red-result.json`。
- 最终绿：`final-results.json` 逐条保存实际命令、二进制 SHA256、退出码与 wall。五套并发执行，`day_end_public_views_tests` 29/29（0.295s）、`desk_semantics_tests` 40/40（1.700s）、`decision_tests` 9/9（0.230s）、`desk_projection_cache_tests` 1/1（0.251s）、`day_end_tests` 8/8（0.233s），共 87 个唯一 case 全过。各命令外部 10000ms，harness 32 threads，没有长跑普通测试。
- 构建：`build-red-valid.*` 29.37s、`build-error-red.*` 23.24s、`build-final.*` 24.06s；全部 `cargo test --manifest-path .worktree/finish-125-price-cage/Cargo.toml -p engine --lib --no-run --offline -j 128 --message-format=json`，环境 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_TARGET_DIR=<主区>/.tmp/build-cache/finish125`，统一由 `node scripts/run-long-validation.mjs 300000 --` 启动。初始准备构建 74s，另有一次测试 helper 尚未移植的编译失败 `build-red-final.*`；后者不算业务红证，完整日志保留。
- 源码绑定：`final-source-hashes-before.json` / `final-source-hashes-after.json` 包含全部 engine Rust 源与 Cargo manifests/lock；最终测试 runner 校验两份一致。红测试加构造失败测试后只新增该测试，修复后测试内容未再变。
- 多核：128 logical CPU，构建显式 jobs=128；运行时五进程并发，各 32 harness threads。实际构建中通过 `ps` 核对过 engine rustc 为 110% CPU / 8 threads（绿色主修复构建）及 97.6% / 4 threads（红构建）；单 crate stable rustc 前端存在串行限制，不能声称占满 128 核。最终额外采样 `final-build-cpu.txt` 来晚，只匹配到命令自身，**不能作为最终构建 CPU 证据**；没有为了补采样重做已通过的构建。
- `git diff --check` 通过；未全量回归、未正式性能验收。等待未实施 Sol 审完整 diff，不宣称门禁已完成。
