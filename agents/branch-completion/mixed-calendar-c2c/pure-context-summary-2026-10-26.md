# C2-C 本股窄接收校验红绿冻结

2026-10-26 执行，2026-10-27 冻结。该纯面是已批准 C2-C §4 的增量，不是中央生产激活。

## 实际改动

只修改 `session/market_calendar_context.rs` 与该专属 tests。新增 crate-private `MarketDayContext::validate_for_stock` 与 `MarketTickContext::validate_for_stock`；保留 visibility，不新增序列化/持久字段，也没有调用任何生产 caller。

- Day 窄门依次校验相同 Arc source owner、本股 StockId/exchange 点查与日期；closed 本股仍可合法参与协议。
- Tick 窄门复用 Day 窄门，再逐字段核 layout/before；完整目录校验仍留在既有 `validate_for`。
- 提取 owner/binding/date/clock 私有 helper，完整入口与窄入口使用相同错误与优先序。窄入口只做一次 `model.stock` 查找，不构造完整 bindings BTreeMap，不扫描其他证券。

该纯面不改市场交易、T+1、价格、分钟或计划制度；SSE/SZSE 各自休市依据仍沿已批任务。2030差异 policy 明确 synthetic。

## 真实 TDD 与来源

- 编译 stub 与新增9项断言先提交 `f45dcc81`。stub 返回 `Ok(())`，编译成功后9项中7项真实断言红、2项正控绿；不把正控冒红。
- red轻编译0.888625秒，red 9case按8并发共0.163431秒；原件 `pure-red/`，源快照与binary SHA均密封。
- 实现后新增测试文件逐字未改，SHA-256 `940ded77d66c45fc5419d4d2b9b815d41d5b1fecb5c2587ef5a3e4e1823f5699`；新9项加原context13项共22/22绿，8并发共0.152350秒。
- green轻编译0.899936秒；源码编译前后与运行前后SHA相同。实现 source SHA-256 `701ea312fd00c28d867adc5f5c66655cac03ee523929307294609a8d63da8468`。
- green binary `.tmp/mixed-calendar-c2c/pure-context-green`，9089952 bytes，SHA-256 `defb163aa9836d5f7490c23149cd2d50ffb1aaf6f88e609d6dad91e26c116da1`。red binary独立保留，未覆盖。
- harness按本树真实路径重新编译C1a/context/tests；既有domain只读复用root已授权同源6ac normal engine rlib，SHA `9bee5bf8bb46234eaaeb075f82d38dde14ae2c4e9169664669f1627ed88f1480`。不冒称整engine crate重建或native/WASM已绿。
- 每个case与实际整批均用进程外10000ms supervisor，rustc codegen8/lld8，各case独立进程libtest1线程，批并发8。所有任务已正常退出；检查进程表没有遗留本次rustc或test binary。具体原件见 `pure-red/compile-plan.json`、`pure-red/run-plan.json` 与green对应文件。

## 外层命令证据定位

实际外层命令在本agent tool原件中，旧plan未另存outer argv；此处是后续定位说明，不冒称它已存在于旧receipt。

```text
node scripts/run-with-deadline.mjs 10000 -- python3 agents/branch-completion/mixed-calendar-c2c/pure-context-stage-2026-10-26.py red compile
node scripts/run-with-deadline.mjs 10000 -- python3 agents/branch-completion/mixed-calendar-c2c/pure-context-stage-2026-10-26.py red run
node scripts/run-with-deadline.mjs 10000 -- python3 agents/branch-completion/mixed-calendar-c2c/pure-context-stage-2026-10-26.py green compile
node scripts/run-with-deadline.mjs 10000 -- python3 agents/branch-completion/mixed-calendar-c2c/pure-context-stage-2026-10-26.py green run
```

node实际绝对路径为根 `.tmp/node24-env/node`，cwd为本独占worktree。对应tool chunk/session分别为 `8521f8/85256→6e5a6f`、`c0fbca/70463→d9d5bf`、`8b2153/59100→2d31a4`、`2b00a4/9561→a9d951`，均exit0。失败case的JSON仍诚实记deadline CLI wrapper1，stderr保最内层101文本；没有为改收据重跑。

本批等待未实施者独立复核完整diff，不单独合main。初始市场8项行为红尚未修复，中央接线仍等待INFO可信冻结后的唯一staging OID。
