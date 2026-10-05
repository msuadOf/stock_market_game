# Simple fixture 与 hash 测试迁移

## 最新蓝图下的冻结状态

已连续读取最新 `q14-financial-model-design.md` 的 1–225、226–450、451–676
行至 EOF；SHA-256 为
`fee250d43e54065156908496ad37aebe852394cad4d74927c688c5818ad206ff`。
当前 Simple Period 合同是：基本面结算周期可配置为月、季度、半年或年；按同
长度期间的营收与开支生成财务事实，再经会计和税务关系推导利润，不以
`net_margin`、直接利润或目标 ROE 作为生成输入。收入、固定开支、变动开支等
按明确基准、趋势和各周期扰动配置生成；四种结算周期分别配置 noise，不把月度
扰动原样当作季度、半年或年度扰动，也不强制内部按月抽样。期间基准必须与
目标期间长度一致，不能把月金额乘季度增长系数伪称季度金额；零营收也不会仅靠
增长率自动复业。

初始化允许以股价、总股本和明确标注为虚拟假设的估值倍率一次性反推基本面初值，
但不是通用配置要求。既有旧测试 fixture 显式给出经济金额、并不从股价反推，
仍然合法；价格反推不扩展到后续结算、读档或旧 fixture 迁移。Simple 的账面
现金只作展示，不代表被追踪的真实公司资金流；这不影响共同财务查询和股本行为
契约。正式设计依据为 ADR-0035、ADR-0036 与 Q14 蓝图。

以下迁移说明记录旧测试 fixture 与 hash 合同的保留边界，不表示所有旧测试、
新 fixture 或端到端经济路径已经通过。既有 11 项 Simple Session hash 与 8 项
低层会计断言继续保留；`extraction_replay.rs` 三个 pinned FNV 未修改，历史
hash / extraction 证据债务继续存在，不能重算或删除锚点来冒充通过。

当前可报告的验证边界：host64 五个 Rust 包 `--lib --no-run` 编译成功；七组
短测共 69 项，分组为 15、10、6、11、10、9、8 项；typegen 128 项通过；Server
availability 1 项通过。它们仅证明对应编译与短测范围，不代表 Q17、fixture
迁移、所有旧测试、完整回归或三宿主运行验收通过。先前 `host54 --lib --no-run`
记录的 41 个编译错误属于历史失败，不是当前结果；修复重复 macro scope、测试
引用和低层 fixture 的过程记录保留作历史事实。

## 范围与状态

本批在 main 工作树迁移 Rust `SessionSetup` 的独立 fixture：以明确的
`company_system: CompanySystemConfig::Simple(SimpleConfig)` 代替旧
`company_operations` / `groups`。共享 `test-support/simple_company.rs` 仅由
test / example 的 `include!` 引用，生产模块中的引用受 `cfg(test)` 限制；
没有新增生产默认、serde 缺省或兼容旧模型的路径。

此处关于 fixture 字段的描述沿用迁移时的旧输入形状：当时记录了收入、可选权益、
增长、`net_margin`、环境与公司扰动参数。金额沿用 `AccountingAmount` 分，
发行人 ID 按股票代码逐项匹配；经济参数不由股价、证券类别或股本反推。该
fixture 保持显式金额输入，符合现行合同；
从价格与虚拟估值倍率反推属于可选的一次性初始化能力，不能据此要求普通测试
fixture 改成价格锚定。这些数值只是虚构测试输入，不是经济校准或生产默认。
利润应由期间营收、开支及税务关系推导，旧迁移字段中如有 `net_margin` 只表示
历史输入形状，不是现行 Simple 合同。股票集合发生增删、替换或代码调整的有效
fixture 同步更新公司配置集合；非法空股票与重复股票输入仍保留原验证断言。

本次记录整理没有重新运行 Cargo、测试或编译，也没有修改 index、提交或总账；
上文验证结果来自已有 host64、typegen 与 Server availability 记录，不是本次
整理产生的新证据。`git diff --check` 通过仅说明本文件差异无空白错误，不等于
测试通过；本文件仍须由未参与本次编辑者复核。

## hash 断言分层

以下八个旧测试的会计断言迁至 `financial_hash_contract_tests.rs`，直接构造
`CompanyRegistry`、`CompanyOperations`、`ClosingEngine` 和 `PublicLibrary`。
fixture 不创建或运行 `GameSession`，不能冒称 Simple 仍执行行业会计：

| 旧测试 | 保留的有效边界 |
| --- | --- |
| `ordinary_tick_shadow_shares_report_histories_and_isolates_mutation` | Arc 共享、closing 写时分离、原始 projection 不变、library 仍共享 |
| `tick_shadow_shares_immutable_company_registry_without_changing_hashes` | registry Arc 共享、会计 projection 与完整序列化一致 |
| `company_operations_shadow_shares_journals_until_an_actual_change` | ops Arc 共享、真实 flow sequence mutation 分离、原 projection 不变而副本改变 |
| `company_operations_hash_cache_is_invalidated_by_authoritative_mutation` | 重复 projection 稳定，真实 market shock 改变并刷新 projection |
| `company_operations_projection_is_stable_across_repeated_hashes_and_shadow_clone` | warmed clone 隔离、不同真实 shock 不同 projection、serde 恢复保持结果 |
| `company_operations_projection_survives_serde_round_trip_without_serializing_the_cache` | ops 完整事实与恢复 projection 相等 |
| `closing_hash_cache_is_invalidated_by_authoritative_recording` | 真实 report recording 改变并刷新 closing projection |
| `closing_projection_clone_mutations_are_isolated_and_serde_stable` | warmed clone 隔离、不同 report recording 不同结果、serde 恢复保持结果 |

`hash_contract_tests.rs` 保留 poison、诊断缓存、权威计数器、retail 游标、
public library 混合 digest 与 tick commit 的既有断言。新增五个 Simple
测试覆盖：选定系统及公开历史的 Arc / 双 hash 共享；零收入公司通过真实
`RestartRevenue` 指令触发 COW；指令与自然日推进刷新 hash；两种收入的
独立 mutation、shadow 和 Session 保存恢复；`CompanySystem` serde
保持事实 / projection 且不序列化 cache。原 Session 的 business / session
双 hash 断言由 Simple Session 测试继续覆盖，低层会计 fixture 不伪造这两个
Session API。

## 尚待协调的旧模型引用

- `company_event_contract.rs` 的 shock 公告断言需要保留在低层披露测试。
- `company_scenarios/lifecycle.rs` 的真实年度三表与完整会计恢复断言需要迁低层；
  `controlled.rs` 的 issuer spec 查询需要换成当前发行人身份入口。
- `save_contract/main.rs` 的经营 scheduler / date 断言需要拆出；
  `save_contract/failures.rs` 的旧财务根字段缺失守卫需跟随明确契约更新，
  并在低层保存原财务字段严格性断言。
- Bank、Insurance、scale restore 与三份行业 / 集团会计测试由其他 owner 迁移。

## extraction replay 基线

`extraction_replay.rs` 的三个 pinned FNV 完全未改。Simple 会改变经营与公开
信息事实，不能以删除 JSON 字段或重算新数字宣称与旧会计 Session 等价。
旧模型的三个原锚应继续在冻结旧实现及其原 fixture 上复现；当前 Simple
fixture 要独立 capture 并由非作者复核完整事件、现金 / 股份守恒、费用、
T+1、真实成交与恢复续行，再明确建立新模型基线。当前没有做 capture，
因此不宣称该 replay 已通过或新旧经济轨迹保持相同。
