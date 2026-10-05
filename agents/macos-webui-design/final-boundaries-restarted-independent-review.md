# 文件读取屏障与财报 gold 的重新独立复核

复核日期：2026-10-05。reviewer：本轮新建的 `independent_review_restart_sol_high`，`gpt-6.1-sol high`，未参与产品实现。本次自行读取规范、完整 tracked diff 与原始证据；未读取旧 reviewer 的结论文件，不以旧结论替代审查。

## 范围与结论

基线为 HEAD `5b6750758787ed63380dc4f72d1245ca229d3e71`。覆盖当前九个 tracked 文件的完整未提交 diff：`DESIGN.md`、`UX-CONTRACT.md`、两份需求/终端工作记录、`save-file.ts`、`file-target.test.ts`、`useSaveCommands.ts`、`save-commands.test.ts`、`company-information.spec.ts`；同时读取新增 `company-causal-audit.md`、`company-causal-probe.rs`、三个有效报告 JSON、setup 与有效构建日志，并核对隔离源码归档。最后纳入主 agent 对方法名和本轮审查归属的文档修正。

**本批独立门禁通过：未发现未解决的产品代码阻塞问题。** 两个文档准确性发现均已由主 agent 修正并由本 reviewer 再次核对。此次结论只关闭文件读取屏障与首份公开财报 gold 两个边界，不能关闭整个终端 goal；手机范围切换后顶栏仍显示自选的问题属于下一批。未纳入其他历史未跟踪截图/失败归档，没有操作用户游戏、加载私人存档或提交玩家委托。

## 门禁一：大 A 语义与依据

本批没有修改 engine、撮合、申报、T+1、费用、证券类别或存档 schema。读取屏障保证文件读取等待已经进入 `DayEndPersistence` 的写入，与 ADR-0025 的完整自然日日结、不可变候选、失败保留上一有效档和旧 generation 不覆盖新局契约一致。仍执行原来的严格存档解析和 `validateDayEndArchive`，未把日内状态开放为持久化入口。

`useSaveCommands.ts:68` 和 `:181` 将同一个 `DayEndPersistence.beforeRead` 传给恢复/文件读档。FS Access 在 `save-file.ts:182` 同步启动 picker，用户选中后于 `:191` 等待，再于 `:192` 获取 File；Tauri 于 `:120` 等待后读取路径；upload 于 `:219` 等待后调用 `file.text()`。三者没有新增队列或新的存档事实。FS Access 的取消 catch 只包住 picker（`:181`–`:189`），屏障、`getFile` 和文本读取中的 `AbortError` 会进入错误路径，不会被冒充取消；两个命令的既有 catch 将错误展示为 notice。

财报金额继续使用 `AccountingAmount` 的元字符串，未混入 `Money` 的分字符串。基准差异来自仓库已经提交的经营期末调用：读取 `36b6f83` 与其父提交的 `day.rs` 可见 `OperatingDayRun::advance` 增加 `settle_period_end`。正式 `docs/company-accounting.md` §2.6 已登记自然月末直线折旧、年末计税与到期收付的游戏简化，`docs/trading-rules.md` 也明确公司经营与投资者现金的分离。本次只验证该游戏契约和测试基准，没有重新取证现实会计准则或宣布现实折旧/纳税制度合规；本批未引入新的交易制度，因此不需要以新法源替换既有规则依据。

## 门禁二：需求必要性与最小范围

文件读取适配器增加一个可选 callback，加载命令只补两处转交，保留先打开选择器的 user activation。选择取消不进入屏障，成功取得并验证档案后才沿原路径 invalidate。新局没有产品逻辑改动，只补旧写入失效后等待退出的回归测试。文档中的行为约定与代码一致，未新增依赖、状态 owner 或存档版本。

E2E gold 更新有独立因果证据，不是根据当前实际结果削弱断言。本 reviewer 使用标准库 JSON/Decimal 独立核对：

- `before` 与 `counterfactual` 的整个 JSON 对象相等，包含 report 身份、日期、全部财报及附注。
- 与 `git ls-tree/git show 20e160b` 对照，隔离归档共有 600 个文件；不同的只有 `Cargo.toml`、`Cargo.lock` 和 `packages/engine/src/company/operations/day.rs`。前两者是隔离 workspace/依赖闭包，业务源码仅省略期末调用并加审计注释。
- setup 与当前 `DEFAULT_SETUP` 和 `App.tsx:93` 的 tradingE2E 配置一致：五只证券、零 NPC、Random、30/9/3 tick、2030 起始日期；历史 Money 为整数分输入，当前规范为分字符串，审计记录明确区分表示与业务输入。
- 管理费用增加 `109278690.49`，减值损失减少 `2871190.48`，1602 累计折旧 movement 为 `-109278690.49`；Decimal 精确计算 `12928574075.43 - 109278690.49 + 2871190.48 = 12822166575.42`。

`company-information.spec.ts:74`–`:77` 保留精确净利润断言并增加管理费用、减值损失、1602 附注分项，未删除或弱化已有报表、公开编号、披露日期和金额单位断言。主仓库 engine/WASM 源码没有改动。隔离归档属于本主题因果证据，不属于生产实现。

## 门禁三：边界测试、跨层语义与复杂度

新增短测覆盖 FS Access 提交前不抓取旧 File、成功读到更新后的档案、picker 取消不进入屏障、普通失败及屏障 `AbortError` 不读旧文件；upload/Tauri 覆盖等待与失败不读取；命令测试覆盖 loadFile/recoverFromFile 转交屏障且不使当前写入失效，并覆盖 NewGame 使旧写入失效后等待退出。既有测试继续验证加载失败释放替换门禁、错误展示、基线同步与显式重试。本次没有发现必须补齐才能合并的测试缺口、跨层语义漂移或多余抽象。

证据仍有限：FS/Tauri/upload 屏障短测使用 adapter mock，没有实测所有原生对话框或所有文件系统的覆盖/快照实现；upload 的 File 对象来自选择事件，本批约定保证等待后读取 text，不宣称外部程序修改后的 File 能自动刷新。当前上传环境本身不支持游戏重复写入文件目标。完整浏览器绿色不能替代所有原生环境验收，也不能证明任意硬件、全天所有交互组合均通过。

## 本 reviewer 核对的验证与失败记录

本轮没有重复执行完整测试或构建；独立执行只读 diff、源码/JSON 比较、Decimal 复算以及 `git diff --check`。后者通过。

- `final-boundaries-final-unit.log` 的各 shard pass 数独立求和为 847，全部 fail 为 0；155 文件、8 shard、10 CPU 可用、wall 2173ms 的 runner 摘要与记录一致。
- `final-boundaries-final-short-unit.log` 为 33/33、410.489375ms；`final-boundaries-final-e2e.log` 为 64 passed、50.6s、3 workers。属于已有执行证据，本 reviewer 未重跑。
- production 日志包含 `tsc -b && vite build`、Vite 343ms 和 release WASM verified。typecheck 与 changed-lint 文件为空，是无输出日志；本 reviewer 不把空文本本身当作独立的 exit 状态证明，exit0 沿用主 agent 已记录的实际命令结果。
- premium strict JSON 为 0 finding；全库 lint 原始日志明确五个既有 children-prop warning、exit1，不能汇报为全库 lint 通过。
- 两份产品红日志分别记录提前读取（actual 1 / expected 0）和未拒绝写入失败，包括屏障 AbortError 被吞的失败。审计记录把 mock/类型失败与产品红分开，没有用前者冒充 TDD 红。
- 有效 before/merged/counterfactual 构建日志均包含 Compiling engine；5.45s/23.92s/7.02s 与工作记录一致。最早 PATH、政策身份、mtime 缓存及原提交编译错误被明确排除，未作为有效因果结果。

## 发现与再次核对

1. 文档方法名不准确：`company-causal-audit.md:13` 原写 `OperatingDayRun::run`，实际源码方法为 `advance`（隔离 `day.rs:79`）。主 agent 已改为 `OperatingDayRun::advance`；本 reviewer 再读确认，已解决。
2. 当前审查归属需准确：`terminal-fidelity-plan.md` 最后验证段最初只指向历史 `final-boundaries-independent-review.md`。主 agent 已保留历史记录并增加本次 `final-boundaries-restarted-independent-review.md`；requirements 工作记录也明确此次新 reviewer 独立复核、不以旧结论替代。再次读取确认，已解决。

这两项仅涉及文档准确性，不涉及产品代码；最终完整 diff 与上述文档增量均已纳入本次结论。
