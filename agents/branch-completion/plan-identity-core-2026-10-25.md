# PlanId 独立身份核心批（2026-10-25）

## 公开接口与范围

- `PlanId { owner: AccountId, sequence: u64 }`，数值排序为 owner → sequence；wire 两项均 canonical u64 十进制字符串。
- `OwnerSequence { owner, next_sequence: Option<u64> }`，wire 必填 `next_sequence`，string 或 null；None 为耗尽。
- `PlanBook::from_parts(policy, Vec<OwnerSequence>, Vec<TradingPlan>)` 在建立 map 前查重和校验。
- `PlanBook::owner_sequences()` 按数值 owner 顺序遍历；`into_parts()` 完整保留 policy、terminal 与高游标，供 runtime 下一批接线。
- 新格式 `{policy, owner_sequences:[...], plans:[...]}`；严格拒绝旧结构，无迁移或 fallback。
- PlanRecords 改 16 字节数值 radix 键；group Abort hash 覆盖两个身份分量、policy 和全部游标及 None。
- 本批只改核心、Rust 机械调用点与测试，不实施 runtime 权威播种/路由/完整合并，也不实施 #122 三档策略。

## TDD 与验证

- 基线 f5820291；红测提交 d6c3ff43。
- 新增四项红测首次编译实际失败，出现 OwnerSequence 未定义、PlanId 无 owner/sequence、无 sequence 字段等九项错误；原件 `plan-identity-red-2026-10-25.log`。
- 核心实现后同测试二进制普通执行：外监督10000ms，`--test-threads=4`；4 passed，0 failed，0.00s；原件 `plan-identity-green-2026-10-25.log`。
- 构建单独归类为长编译，外进程 runBoundedCommand 300000ms；CARGO_BUILD_JOBS=16，独立 `.tmp/plan-identity-target`；绿色构建用61s。cold build deps 并行；单 crate Rust frontend 存在工具串行阶段，不能以 jobs 数冒称全程满核。
- 原始日志保留在本目录、受仓库 `.log` 忽略规则影响，未伪装成跟踪文件。完整受影响 Rust 测试编译/回归、bindings 导出及独立复核尚待后续补齐，不能凭本阶段宣称身份前置完整交付。

## A 股语义与错误上下文

本批没有新增交易制度，沿用 trading-rules、ADR-0015/0016/0017/0025/0031/0041/0043 已登记依据；只改内部身份与当前格式，资金分、股份股、T+1、费用与受理 FIFO 不变，未声称重新联网核验交易所规则。

PlanId 错误上下文统一为 owner 高/低 u32、sequence 高/低 u32。现存错误码2141/2142/2143/2144/2183/2184/1042/2185 的身份前缀由一个槽改为四槽，其后字段保持原义；后续将更新正式错误清单。新 PlanError 为领域类型化错误；运行协议映射由 runtime 批实现。

## 最终代码冻结与复核修复（2026-10-26）

代码冻结为 `07349845665609813394ae1d84f8b922b9e27da3`，基线
`f58202911b8db6c042fe1816d08277c16381f2d2`。完整本地提交链按时间为：
`d6c3ff43` → `d3a808d0` → `b9056c4e` → `bab25f82` → `21cbee78` →
`e8a3d314` → `83516718` → `489de2ae` → `c5f1bb20` → `07349845`。
无 merge、push 或 #122 策略实现。

- `e8a3d314`/`83516718` 是 root 委托修补已存在的 feature-only `OrderFillSettlement.gross`
  测试夹具：100股 × 950分 = 95000分；按同一 feature 编译，不改变原行为断言。
- `489de2ae` 增加无当前计划的未知 owner 游标名册校验（fault2690），保留原计划错误首错。
  错误清单已实际追加至 `agents/engine-max-performance/string-free-inventory.md`，
  上文“后续更新清单”的阶段状态已被本段替代。
- 独立 reviewer 发现 session 恢复防御分支仍存在四处 fault1042/2185 旧上下文；
  `07349845` 已改完整身份四槽并加强真实坏child/exposure测试的精确 context 断言。
- `c5f1bb20` 修复一项原全局创建顺序测试：现在精确断言数值 owner→sequence 的存储遍历顺序，
  不删断言、不把该顺序当作受理优先级。原失败记录 `plan-identity-plans-unit-regression-failed.log` 保留。

## TDD 证据的诚实更正

上文首轮九项编译错误**不是** `docs/testing.md` 所要求的业务断言红，不能宣称它满足原始TDD红阶段。
这是真实流程缺口，未回写历史。

独立 reviewer 指出后，新增仅使用既有公开 API 的 `plan_identity_behavior.rs`，在实施后补做
真实旧基线负控：两个独立合法账户各自首建计划，断言序列化身份不相同。冻结旧源码来自
`git archive f5820291`，560个源码文件逐一与该commit的Git对象核对一致；只复用第三方依赖，
未编辑旧源码。当前同一份跟踪测试源码（SHA `60ab547ae02027359fbf3ecef3fc9a4a4f0100482347a1a5aa4f973ab0bd13e6`）
在旧基线实际运行失败 `Number(0) == Number(0)`，在最终候选实际通过。

- 当前源码重编负控原件：`plan-identity-behavior-baseline-frozen-build.log`、
  `plan-identity-behavior-baseline-frozen-red.log`。
- 绑定manifest：`plan-identity-behavior-negative-control.json`；包含基线commit、560文件核对数、
  测试源码SHA、旧rlib及test binary SHA。此负控是实施后补证，不能替代不存在的实施前行为红。
- 新增未知无计划owner游标校验确有实施前业务断言红：`plan-identity-owner-cursor-red.log`，
  此后 `save-contract` 对应严格fault2690及完整owner高/低槽断言为绿。

## 最终限域验证

最终普通Rust构建原件 `plan-identity-reviewed-build.jsonl.log`、`plan-identity-reviewed-build.log`；
`--lib` 与7个受影响integration目标的 no-run 构建实际通过，50.91s。使用进程外共享300000ms监督、
`CARGO_BUILD_JOBS=16`、独立root `.tmp/plan-identity-target`；没有运行全engine/full regression。

最终普通执行：`run-plan-identity-regression.mjs`，外层与每个子命令均10000ms，23条独立命令并行，
每条`--test-threads=8`。`plan-identity-reviewed-results.json` 绑定冻结commit、每条命令、实际binary SHA、
时间与原始日志；**136 passed、0 failed**，最慢子命令5.366s。

覆盖核心身份/完整u64/canonical/Option、terminal/高游标/policy/失败原子性、原plans状态机、
账户内allocation精确预算与排列不变、save语义拒绝、radix COW、完整执行续行、真实child/exposure
恢复拒绝、计划终止/收尾，以及订单簿price/FIFO和股票worker真实supplied arrival。计划级
逆PlanId排序与arrival的真实FIFO正例由runtime批继续补齐，不拿本记录冒充该整体前置验收已完成。

绑定导出独立执行到唯一临时目录，普通10000ms监督，1 case通过，7.32s；原件
`plan-identity-reviewed-export-alone.log`。导出的 `PlanId.ts`、`PlanBook.ts`、`OwnerSequence.ts`
与跟踪生成文件逐字节相同。连同普通行为合计137项通过。

曾有三次聚合runner预算失败，原件保留：首次逐job重复hash巨大unit binary；第二次去重hash仍挤占
普通执行预算；第三次预hash后共享输出的导出case未在聚合窗结束。均未报成通过。最终先独立预hash，
普通聚合明确skip导出，再以独立目录补验导出，没有放宽10秒上限或弱化业务断言。

## 真实生产 wire 与 Web 往返

`plan-identity-producer.rs`（源码SHA `b4f52ef75cfe10fc4372abaa25974a4b1aa06c83fa6943d41409bdad083a9faf`）
通过真实 `GameSession::new` → 两日 `step/end_civil_day` → `save` 产生完整JSON；没有手改字节或注入计划。
源程序先对同一原始字节执行 `decode_save_slot` 与 `GameSession::restore`，成功后写证据。
最终073 rlib重新构建并执行通过，`plan-identity-reviewed-producer.json` 绑定源码、rlib、producer binary与存档SHA；
编译外监督300000ms，执行外监督10000ms，原件 `plan-identity-reviewed-producer-build.log` 与
`plan-identity-reviewed-producer.log`。

产物1819353字节、两个真实计划（不同owner均sequence0），原档SHA
`03a7e1776a5b81e09cf6bd91b365f4eeea1229b5b4c9cf470c7653739561ca24`，位于root
`.tmp/plan-identity-target/producer-save.json` 与 `producer-save-reviewed.json`（二者相同）。
Web worker对原完整JSON严格parse、deepEqual、重序列化再parse通过，未删字段；输出
`.tmp/plan-identity-web-receipts/roundtrip-save.json`（位于Web独占worktree）SHA
`e5541bd1e4fd6a3f4aab201095e98176a5b6721a0226731354c9425e9ef534fe`。
本核心用 `plan-identity-web-roundtrip.rs` 再对Web输出执行生产Rust decode/restore，实际通过且计划数2；
原件 `plan-identity-web-roundtrip-green.log`，外监督10000ms。

## 仍需整体收口的事项

- `company_scale` 的feature短case旧StockCode错误fixture及缺catalog由独立diagnostics writer修补；
  必须在叠加完整身份代码后实际通过原fault2142/2143断言。普通137项不覆盖它。
- runtime播种/归属/完整合并/恢复与计划级arrival正例由另一writer完成，本批没有宣称这些已交付。
- 最终可信独立审查与整合前置终裁由root安排；本记录不是自我批准。
