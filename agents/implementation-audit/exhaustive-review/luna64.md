# hosts 状态与复核记录 EOF 全文复查

日期：2026-10-03。基线 `b89afb3346743a4b4fccf26c9ac9ff108595f696`；当前产品 merge `a7c7ce3`（含产品 `08e4fc7`）。本次只读审查指定三份全文及其所指源码、调用方和正式交易规则；未运行测试、Cargo、编译或性能矩阵，未修改产品源码及 Git 状态。唯一写入为本审查记录。

## 全文/章节矩阵

| 文件 | 行数 | 章节覆盖 | 原结论复核 |
| --- | ---: | --- | --- |
| `hosts/performance/status.md` | 59 | L1-12 实施/action/caller；L14-29 验证证据；L31-35 语义和限制；L38-50 独立复核修复；L53-55 最终复核；L57-59 root 代表性验证 | N07、R2-N18/N21/N22 的 owner 与 caller 在当前实现存在；旧复核 PASS 仅覆盖其记录范围。L9 明确未修 sampler rejection 的 child 清理；该限制与 L59“没有未关闭实施或独立复核发现”不是同一含义，不能推导风险已修复。 |
| `hosts/review-account.md` | 39 | L1-9 范围；L11-15 三门禁；L17-25 重点 caller/结算；L27-30 记录修复/范围；L32-39 文件哈希 | 适配测试 caller 的静态断言等价结论成立；它不审计整个 Account 实现或所有生产 setter。getter/caller、grant_position、settlement 确有真实实现；无需据此改领域语义。 |
| `hosts/review-actors.md` | 71 | L1-7 结论；L9-24 范围/依据；L26-45 语义、最小范围、owner；L47-65 边界；L67-71 检查与剩余验证 | Pacing 私有 owner 与宿主命令 caller 真实存在；静态审查 PASS 有明确边界，L65/69-71 明说 Rust 类型/行为及 doctest 未完成。status.md L59 仍将 actors API doctest 列为未选中，不构成相反证据。 |

## 实现链与原文证据

### Performance sampler 与 child 所有权

`status.md:9` 原文称：

> “私有 ProcessSampleRun 持有 child、stdout/stderr、spawnError、起止时间、RSS、thread samples、timer/release；……sampler rejection 的既有 child 清理问题未修复”

当前 `scripts/simulation/escrow-performance-harness.mjs:325-420` 确实由 `ProcessSampleRun` 私有持有 child 和采样状态；真实入口是 `runProcessSample(endpoint, options) -> new ProcessSampleRun(...).start()`，`runPerformanceHarness` 默认将它作为 `runSample`，CLI main 再经 harness 走到此 facade。`start()` 注册 close 后记录 `endedAt`、停采样，再 `await sampler` 并生成 measurement；正常 child close 的停表语义与 L9、L41-44 所述吻合。

但下面这条具体链仍保留：

```js
const sampler = this.sampleTree();
...
const { code, signal } = await new Promise((resolve) => this.#child.on("close", ...));
await sampler;
```

`sampleTree()` 中 `await this.#sampleTreeProbe(...)` 的 rejection 没有本地 catch/finally 去停止 child；`start()` 先等待 child 的 `close`，之后才 await sampler。若 probe 在 child 仍运行时拒绝，owner 没有及时终止/清理该 child/tree；promise rejection 也可能先成为未处理 rejection。这个缺陷虽已在实施状态中披露，却仍是**未修复的有效残余**，旧 performance reviewer 对抽对象/已声明边界的 PASS 不构成修复证据。建议作为独立行为修复补有界 fixture，明确 sampler rejection 时及时停止 child/tree、保留原始错误并清理等待 timer；不要在当前审计里宣称性能批次无残余风险。

其他细节核对：`stopSampling()` 清除 timer 并 release delay waiter，正常 close 后 wall 使用 child close 时刻；`collectResult()` 显式抛 spawn/nonzero/no-positive-RSS。`PerformanceComparisonRun.#measureSide` 在源指纹前后校验和 sample validator 后才返回，私有 `#appendSample` 深复制；真实 caller 是 `runPerformanceHarness` 每轮新建 run，`main/loadReusableReport` 用同 config 比较报告。其 JSON 吞吐仍标为 committed ticks / wall，RSS、Linux R 状态及 Rayon capacity 分列。旧 review 对这部分字段/样本隔离 PASS 与当前代码吻合，未发现新领域语义漂移。

### Market UI owned resources

`status.md:10` 原文称 main 在 finally 单次 `close`，清理顺序 client → browser → server → profile；异常会截断后续步骤，未加错误聚合、幂等或重试。当前 `MarketUiReportRun` 实际私有持有这些资源，`main` 启动时分别调用 startServerIfNeeded/launchBrowser/connectPage，finally 中 `await run.close()`。`close()` 代码依序执行 `client.close()`、`stopProcess(browser)`、`stopProcess(server)`、删除 profile，无逐步 finally；任一步 rejection 会跳过余项。状态表已准确记录这一现状及精准 fixture；属已知清理失败残余，不是本轮新引入的“已解决”结论。未启动真实 Chrome/Vite，不能从 fixture 推断外部进程集成行为。

### Account 的真实 setter 与 caller

`review-account.md:14` 将本批范围限定为四个测试文件 API 迁移；L19-24 给出边界举证；L30 又明确不覆盖 Account 生产实现。当前 `Account` 生产 setter/变更口包括 `set_strategy()` → `restore_strategy()`、`grant_position()`，以及 crate 内恢复 `restore_balances()`；结算代码在全部 checked 计算后写 cash/positions。调用面包含 engine session 的 `account.cash()/positions()/strategy()` 只读读者、AccountBook/session 下的真实状态写者和上述 tests。这里的有效新反证是范围限定本身：不能把四个 caller 测试文件 77/13/12/2 个断言等价，外推成 Account 所有 setter/调用面或生产状态迁移已独立审完。原报告已主动声明这一点，因此不记为实现缺陷；本轮没有发现这批 API 迁移造成金额分、股数、T+1 或结算顺序改变。

正式规则 `docs/trading-rules.md` 规定人民币/股份资源、T+1、现行沪深差异及明示简化；本批审的是测试适配/工具 owner/Pacing owner，没有改动交易制度。此处沿用正式登记，不声称重新核验交易所条文。

### Actors 的真实 setter 与 caller

`review-actors.md:41` 记录 Pacing 按值拥有 `running/fastest/requested_speed/tick_interval/base_ms/SpeedMeter`；当前桌面 `DesktopPacing`、服务端 `ServerPacing` 均有这些私有字段与真实转换方法。宿主 `SessionActor` 命令处理实际调用 `pacing.apply_speed`、`set_running`、`pause_at_civil_boundary`、`stop_after_failure`、`reset_after_restore` 和 `refresh_metrics`；桌面 Tauri `set_speed` command → handles → mpsc → actor，server `api_speed` → handles → command → actor。`SetSpeed` 是桌面 fire-and-forget，而 server 有 oneshot 结果；保留宿主差异符合 ADR-0010/0017，而非跨层漂移。

对照旧 PASS：L53-63 的 Fixed 下限、Fastest 切换、Civil pause、Restore、Fatal、速度校验、订阅与 Harness 边界在源码上能找到相应 owner/caller。L65、L69-71 明确静态结果不等于 doctest/行为验证；status L59 也列 actors API doctest 未执行。未发现可据 EOF 文本认定为已通过的漏测被掩盖；全量 host 测试仍不能冒称跑过。

## 结论与候选项

- Account 与 Actors 两份旧 PASS 在各自限定范围内没有被当前 EOF 源码反证；其范围外不作推扩。大 A 规则没有被这些重构改写。
- PerformanceComparisonRun owner、真实调用方及报告字段与状态记录相符。MarketUiRun 的失败时截断后续释放是已披露且测试覆盖的限制。
- **新候选/仍有效项：N07 sampler rejection 时 child/tree 清理缺失**。它被 status 原文明示为“未修复”，实现路径可直接复现推理；应作为开放行为缺陷跟踪。此前 PASS 只复核本次抽象边界，不关闭它。
- 本轮未运行短测试，也未验证任何实际性能数字；未审的全量剩余 Rust/API 测试仍按原状态未验证。
