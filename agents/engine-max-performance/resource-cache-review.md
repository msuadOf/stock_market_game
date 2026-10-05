# 资源校验缓存独立复核

复核对象：当前 worktree 完整 diff，包括 `account_book.rs`、`decision_resources.rs`、对应测试、`packages/engine/examples/max_speed_probe.rs` 与 `browser-probe.mjs`。复核不修改实现文件。

## 结论

未发现会改变 A 股交易语义或导致当前缓存命中错误校验结果的有效缺陷。缓存只复用 `DecisionResourceSnapshot::seal` 中纯校验闭包已成功的结果；资源快照及撮合逻辑没有被替换。当前实现可作为性能优化继续评估，但不能据此宣布 16 核达到 1000x 或任务验收完成。

## 检查结果

- **A 股语义与依据：** 本次改动没有更改交易制度、价格、资金/股份预留公式或成交路径。`DecisionResourceSnapshot` 仍从市场 `last_price` 与 live ledger 构造事实，并检查现金预留、可卖股份及 equity 溢出；新增页缓存仅省略相同输入下重复执行的校验。此处不引入新的交易规则主张，因此无需新增交易所规则依据。
- **必要性与范围：** 缓存位于 `AccountPage`，复用现有 COW 页共享边界；资源价格和按账户预留被纳入 key，账户页通过 `get_mut`、`insert`、`values_mut` 统一失效。调用点只替换原有逐账户 Rayon 遍历，改动范围与定位的重复资源验证相符。测试先建立有效缓存，再修改价格触发 equity overflow，能防止旧成功结果掩盖错误。
- **失效与错误边界：** `AccountPage::clone` 不复制缓存，写入前的 `Arc::make_mut` 会在被修改页上重建缓存；未修改页可继续复用。页锁串行化同页并发检查，账户数据在 `&self` 校验期间不可变；Mutex poison 会转为显式 `InvariantViolation`。校验失败发生在写入新缓存之前，失败结果不会缓存为成功。现金与卖出预留逐页参与 key 比较，未知账户/证券的账本检查仍在缓存遍历后执行。
- **测试证据：** 工作记录中的 `resource-cache-red.log` 显示新测试在实现前因重复调用而失败；`resource-cache-green.log` 显示测试通过。`decision_resources_tests.rs` 在预置成功缓存后改变价格，并确认溢出仍返回错误。测试输出均为短测；本复核未再次运行测试。
- **Benchmark：** Native 探针用 `GameSession::step_with_phase_timing`，其实现调用公共 `step()` 并只在 tick commit 后成功返回；测量循环覆盖完整 engine tick，且纳入必要自然日推进。浏览器探针通过实际 Chromium 页面和生产 `createWorkerHost`/WASM worker 启动 session，按宿主 `TickBatch.frames` 的已提交帧数计算速度，因此测量了 Worker 宿主和完整 tick。它没有把 React/render 消费列入计时；`hardwareConcurrency` 被固定为 16 且显式请求 16 worker，报告还记录了 `crossOriginIsolated`。探针含固定 Chromium 可执行路径，跨环境复用时需调整路径。现有样本为约 21.7x，远未达到 1000x。

## 需留意但不阻断本次缓存

`AccountBook::validate_resources` 接受通用 `validate` closure，但缓存 key 不标识 closure 的语义；当前生产代码只有 `DecisionResourceSnapshot::seal` 这一种 validator，故本 diff 下不会发生错误复用。若未来增加另一种调用方，应把缓存收窄为专用校验函数，或将 validator 语义纳入失效契约。

工作目录还存在未跟踪 `node_modules` 与大量性能记录/生成物；这些不属于本次逻辑实现 diff，提交前应确认只保留有意归档的工作证据。
