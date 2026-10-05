# Simple 期间生成与 Web 接缝最终复核

复核身份：未参与本轮实现的独立复核者。复核日期：2026-10-06。

## 结论

本次限定范围 PASS；未发现需阻断的有效问题。该结论只覆盖 Web Simple 的初值预设、seed 草稿、结算周期、期间增长／财务存档校验与公开来源展示，不表示公司系统整个目标已完成。

### 大 A 语义

- 本批算法只生成明确标注为虚拟的汇总公司财务数据，没有改动沪深市场的证券类别、价格单位、撮合或交易规则，因此不需要为本批算法增补交易所制度依据。
- 初值 helper 只读取股票期初价格和总股本，以明确虚拟的 PE／PB 倍率生成 Simple 初始财务；趋势推进不读成交价，也不承诺开盘无跳变或生成共同正确股价。默认值采用虚拟假设，不冒充真实公司统计。
- Web `annualGrowthFactor` 的 1e9 定点根、半偶舍入及金额应用与 Rust `GrowthFactor` 的算法及边界一致；已核对月、季、半年、年度因子和负增长/i128 边界断言。
- Simple 汇总凭证使用 `SimplePeriodSummary` 与 `NonCash`，没有伪造公司实际收付款；公开附注说明 `source` 且明示不暴露未披露模型状态。收入、期间、金额单位和自然周期均与蓝图定义一致。

### 必要性与最小范围

- seed 草稿让用户预览的 seed／公司预设与新局实际消费值保持一致；preset 更改 seed 或结算周期时重建同长度基准，custom JSON 不被 seed 编辑静默覆盖。这些接线是用户明确允许初值价格锚定后使该锚可见、可复现且可编辑所必需。
- Web 严格解析器新增 Simple 配置、运行状态和期间历史契约；删去旧 monthly-generation 代际，不提供隐式兼容。期间财务仅用汇总账簿，收入费用明确入账，不引入 `SyntheticFunding` 或真实公司现金流。
- `IncomeTaxPosition` 由共享税务 owner parser 校验，Simple `recognized_periods` 覆盖开账日至财务截至日，期间凭证为 NonCash；这与共同报表／税务所需事实相符。
- 本次未发现与需求无关的新增依赖或 UI 业务分叉。股价只用于初始化锚定；之后无反向读价路径。

### 边界复核

- 复利算法核对 Rust `packages/engine/src/company/simple/growth.rs`；Simple 状态、趋势、期间与财务边界核对 `period.rs`、`state.rs`、`finance_validation.rs`、`finance_posting.rs` 及相关 Web schema。JS `TextEncoder` 字节排序与 Rust `BTreeMap` 公司顺序语义一致；必要 nullable 字段、精确字段集、复业零基数及历史金额连续性均有校验。
- 预览 helper 对规范 seed、非法价格／股本、周期、零权益舍入明确拒绝；预设更改 seed 重算，custom JSON 不自动覆盖，启动实际消费展示值的接缝已有另一名复核者 PASS 记录于 `seed-preview-runtime-review.md`，此处未重复重建网络 E2E。
- 定向短测共 35 项通过：初值与 seed 8 项、年化增长与期间生成 6 项、Simple 配置／财务／状态 18 项、公开来源渲染 3 项。命令均从 `apps/web` 执行，使用 `scripts/run-with-deadline.mjs 10000`、Node case timeout 10000ms，测试进程并发 4。另用超出 u64 的 RNG 文本作负控，Web schema 按预期拒绝。

## 未覆盖与限制

- 增量静态复核了 `complete-save-schema.test.ts`、`day-end-archive.test.ts` 与 `web-host-integration.md` 的完整差异和文本。Simple 完整槽的发行人、财务账簿、生成金额与 RNG 断言已映射到 `company_system`；已退役 Simulation flow 不再伪装成 Simple 槽字段，四种行业 flow 的字段拒绝及 Industrial inventory 结构拒绝断言改为直接调用其低层 parser，未发现断言被弱化或删除。原四行业各自不同代表字段和错误路径仍被逐项断言；旧的 Industrial chart nested damage 检查现由 Simple `Books.chart.accounts` 分支覆盖。
- 日终休市正例依赖 root 生成的 `current-closed-day-save.json`，并精确检查零 tick、时钟与公司推进日一致以及 `Closed` 覆盖；时钟错位、覆盖缺项、活动委托／资源、未处理请求和父单等负例仍保留。NPC 请求负例现在从真实日终档确认 `pending_npc === null`，再单独构造一个与快照 tick 对齐的非空 batch 测拒绝，不再假设真实 fixture 含空 batch。当前 `current-schema-save.json` 仍缺新 `company_system` 字段，`current-closed-day-save.json` 尚不存在；报告中的 `.tmp/company-system/simple-save-contract-red.log` 因此只能作为预期中的 stale-fixture 失败记录。此次仅静态复核，没有把这些迁移测试报告为绿；生产真实 fixture 应由 root 按既有生成流程刷新，不手工改写。
- 新增 `company-config-commands.test.ts` 的 seed 初值正例已静态核对：通过生产 `createSaveCommands().newGame()` 捕获被安装的 `SessionSetup` 与 seed，并逐股票比较预览中的营收、固定开支和期初权益；设置替换前使用真实 `createSeedDraft` 生成预览。它能证明命令层消费展示配置而不再反推，不单独模拟 `App.tsx`／真实 Host 启动；`App.tsx`、Remote 与 lifecycle 的接线范围已有 `seed-preview-runtime-review.md` 记录。交接文档列出的独立文件和共享文件增量归属与当前路径相符，并明确排除 generated bindings、生产 JSON fixtures、engine／三宿主与股本行为；报告仅声称该定向组 13/13，通过日志 `.tmp/company-system/initial-preview-final-green.log`，未宣称完整回归完成。未发现新增 finding。
- 未运行完整 Web 测试批次、完整回归、Cargo、索引生成或构建。当前共享工作区仍有并行批次的大量未提交改动；保存 fixture／生成类型等外部契约没有在本复核中整体刷新，故不宣称这些验证通过。
- 初始 `price × shares → virtual PE/PB → fundamentals` 是用户新授权的初始化锚点，不代表初始撮合价由基本面强制稳定；不保证开盘无跳变。
- 每公司股本行为偏好仍由另一工作项负责接入，本范围的 PASS 不代表偏好配置或所有公司行为目标已完成。Simulation 模式仍未实现，不宣称两个模式已具备完整共同能力。
- A 股分红、配股、增发、回购的法定方案条件与制度核验不属于本次 Web 期间模型复核；不得把此处 PASS 用作这些规则已按官方现行依据验证的声明。
