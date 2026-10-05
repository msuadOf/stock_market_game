# `company_scenarios` 消费者迁移独立复核

## 范围与结论

复核对象为 `packages/engine/tests/company_scenarios/` 从 HEAD 到 worktree 的完整差异及迁移后全文。仅审查，不改代码、Cargo 配置或 index。当前验证状态按委托说明为“仅编译，未执行”；本记录不将其表述为 full regression。

迁移方向整体符合 ADR-0035：场景消费者由旧 `company_operations`/`groups` 切换到显式 `CompanySystem::Simple`，发行人股本信息从 `company_system.issuers()` 取得；读档后事件及完整序列化存档逐字节一致的恢复断言仍保留在 `restore.rs`。初审指出年度真实账簿断言被公开报告查询替换；增量复核确认作者已从实际 SaveSlot 序列化状态恢复 `SimpleFinanceState`，比较 standalone 2030 年度 `ClosingEngine` 版本数，并把未来经营义务断言迁至真实 `CompanyOperations` maturity 场景。两项有效发现均已处理，复核通过。

## 发现

### 已修复：年度场景恢复实际 Simple Books 验证

增量复核时，`lifecycle.rs` 新增 `annual_close_version_count`：从实际 `SaveSlot.company_system` 序列化值读取该公司的 `finance`，反序列化为 `SimpleFinanceState`，并查询 standalone 2030-12 annual `ClosingEngine::versions`。年末日结前后断言版本数增加，JSON 输出也重新对应年度版本数。因此，该断言验证的是实际保存的 Books/ClosingEngine 历史，而不是从公开报告存在性推断账簿状态。

之前单独提出的 future obligations 断言已迁至 `company_operations/maturities.rs` 的年末场景，使用真实 `CompanyOperations` 日结后断言 scheduler 非空，且每个 due date 严格晚于年末日期。该迁移尊重 Simple Session 不持有旧 Simulation 经营影子状态的边界。旧 Session 字段耦合断言原文保存在 `legacy-session-annual-close-reference.md` 作为迁移证据，不再被当作现行契约。

### 补充检查：股本、金额与报告范围

此项应区分迁移保留与额外增强。旧场景已有 standalone `ScopeId`，具体年度 `AccountingPeriod` 的年结版本增量、公开 annual period 及资产为正断言；新测试保留这些相关覆盖并恢复 standalone ClosingEngine 的年结版本验证。`main.rs` 保留证券股数 fixture，`controlled.rs` 按共同 issuer 接口读取 `issued_shares`。旧场景并未断言发行人股本与证券总股数一致、固定报表金额或按股本计算的金额，因此这些不是本次迁移回归缺口，不应为复核而要求扩展大测试。若后续需求涉及股本行为/金额口径，应由对应领域测试明确规定和覆盖。

### 保留项：不可变及恢复断言

`restore.rs` 保留了 mid-scenario 编码/解码后即时存档字节一致、相同命令下事件一致、日结后完整权威存档字节一致的恢复断言，覆盖不可变历史/权威状态的精确持久化行为。控制测试还保留同一公开报告对不同账户个人信念产生不同结果，以及发布前不可见、读取状态持久化检查。无需因 Simple 迁移弱化这些断言。

## 领域语义与必要性

- 沪深证券板块与 `StockSpec.total_shares` 的既有设置未被本差异改写；迁移没有宣称改变交易制度。无需新增交易所规则依据。
- `CompanyKind::Industrial` 为财务行业类别，不应与证券板块混同；当前 fixture 清晰分层。
- 将测试 fixture 显式改为 Simple 与 ADR-0035 的“先接通 Simple”范围一致；没有把 `Simulation` 静默降级的证据。
- `controlled.rs` 从 `company_operations.company(...).spec()` 改读 `company_system.issuers()` 是必要的共同身份入口迁移；应继续保留 `issued_shares` 的实际消费。
- `advanced_through` 与公开年报断言属于新的 Simple 状态/披露覆盖，但不能据此宣称替代实际 Books 年结验证。

## 验证边界

委托上下文明确说明本批目前仅编译、未执行。本次独立复核未运行测试，不能报告测试通过或完整回归通过。复核通过仅指静态审阅确认上述断言迁移保留了所审查语义，不代表测试执行通过。
