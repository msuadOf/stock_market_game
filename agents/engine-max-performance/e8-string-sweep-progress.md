# E8 无字符串化 sweep 进度存档（滚动更新）

任务：引擎核心全面无字符串化（用户决策，ADR 0041 + string-free-inventory.md）。
本文件保证任何中断后可无缝接续。状态快照时间以最后一条"批次记录"为准。

## 当前总状态

- **库本体（--lib）编译通过，0 错误**（含生产代码全部改完）。
- **测试目标（--tests）剩约 698 个编译错误**，分布见"剩余清单"。
- 约 184 个文件已改，**未提交**（等待清单归零 + 全量编译 + 定向测试后按原计划提交）。
- 完整错误清单：`packages/engine/errors-all.txt`（cargo check --tests 输出过滤）。

## 已完成（生产代码，全部编译通过）

- `src/stock.rs`：StockId(u32)/StockMeta/StockCatalog（id 按代码升序分配；乱序存档读档拒绝；
  Display 打数值）；lib.rs 导出。
- `CommittableSessionState` 增 `stocks: StockCatalog`；`GameSession::new` 构造（`SessionSetup::stock_catalog()`）；
  clone_for_shadow/commit_from/hash 解构同步；markets/price_history/market_minute_closes/positions/
  EnvelopeKey.stock/Event.code/存档结构全部 StockId 化（破坏性契约，无迁移）。
- 公开边界 API：`GameSession::stock_id(&str)->Option<StockId>`、`GameSession::stock_catalog()`、
  `SaveSlot::stock_id`、`SessionSetup::stock_catalog/stock_ids`、
  `CommittableSessionState::issuer_of_stock`（公司域仍 StockCode，翻译在会话侧）、
  `RootReadContext { stocks, issuer_of_stock }`。
- 公司子系统（company/* + company_assembly）**保持 StockCode 不动**（CompanyId 字符串身份
  属清单 C 项，本提交范围切割）；默认公司表 `&'static str` 代码不动。
- 错误枚举 code 载荷数值化（ExperienceError/PriceMemoryError/ObservationError/T1Overflow 等）。
- verification_evidence：投影字段 StockId 化、validate_stock/InvalidStockCode 删除、
  JSON map 键 code.to_string()、EnvelopeKeyProjection/PositionProjection/stock_codes 数值化。
- candles：generate_preset_daily_candles 增 catalog 参数（RNG 种子仍按代码字符串派生，价格路径不变）；
  stock_code_hash 恢复 &StockCode。
- 拍卖哨兵：`StockId(String::new())`/`"<incremental>"` → `StockId(u32::MAX)`（目录 id 从 0 连续，MAX 永不冲突）。
- diagnostics：SeedDiagnostics 增 catalog 字段；ensemble 汇总内联目录翻译。
- persistence 存档校验、civil validate、player_orders/delta、notices、initial_allocation、
  account_validation_context、decision_resources、stock_auction_adapter、auction_tick_transaction、
  ledger/envelope_projection 哨兵——全部编译通过。

## 已清零的测试文件

tests/session.rs（sid()/sid_of() 助手）、tests/account.rs、tests/strategy.rs、
src/session.rs 内联测试、src/session/decision_chain.rs（session.stock_id 配方）。

## 改写配方（继续沿用）

- R1 集成测试单代码 fixture：`StockCode("600101"/"600888")` → `engine::StockId(0)`
  （多代码 pair："600888"=0/"600889"=1；"600101"=0/"600102"=1；"000812"=0/"000813"=1；五股表按代码序 0..4）。
- R2 有 session 的内联测试：`session.stock_id("xxx").unwrap()`；无 session 的纯 fixture 单股 `StockId(0)`。
- R3 **规格构造位（StockSpec.code / stock.code = / spec 赋值）必须保持 StockCode 字符串**——
  这是边界；被 sed 误改的要改回。
- R4 局部 map 声明 `BTreeMap<StockCode,` → `BTreeMap<StockId,`（或 engine::StockId）。
- R5 公司缝隙：`company_registry.issuer_of(&code)` → `state.issuer_of_stock(*code)` /
  `context.issuer_of_stock(*code)`（注意 code 是否引用，StockId 是 Copy，E0614 提示去 *）。
- R6 spec 查找：`find(|s| s.code == *code)` → 先 `let code_str = xxx.stocks.code(*code).expect(...)` 再比较。
- R7 serde_json 索引：`json[..][&code.0]` → `[code.0.to_string()]`。
- R8 断言集合 `HashSet<String>` 按代码集合 → `HashSet<StockId>`（rank 映射）。
- R9 `.0.clone()`（StockId Copy）→ 直接传值；`code.clone()` 可保留（Copy 有 clone）。

## 剩余清单（cargo check --tests，按文件计数，接续时先重新生成 errors-all.txt）

大头：saved_runtime_tests 31、continuous_matching_tests 26、notices 25、verification_evidence/tests 22、
stock_stream 18、account_validation_tests 18、npc_tick_preparation_tests 17、fundamental_beliefs/priors 16、
account_validation_driver_tests 16、continuous_matching_adapter_tests 15、diagnostics 15、
decision_resources_tests 14、npc_decisions_tests 13、live_reservations 13、envelope_projection_tests 13、
stock_auction_adapter_tests 12、pre_open_transaction_tests 12、envelope_projection_hydration_tests 12，
以及其余 ~80 个文件零散项（见 errors-all.txt）。

## 待办（清单归零后）

1. 全量 `cargo check -p engine --all-targets` 0 错误 → 跑定向短测（台账 filter 清单；
   已知基线固有失败照旧不算；hash_contract 因序列化变化需更新 fixture——business_state_hash
   变化是预期内，如实登记不伪装连续）。
2. **不跑 cargo fmt --all**；只对已改文件做最小格式核对（fmt 会沿 mod 树重排无关文件，184 文件在手风险极大）。
3. 提交 A：引擎核心（本批全部）`perf(engine)` 或 `refactor(engine)`：股票数值身份 sweep。
4. 提交 B：结构化错误（wire 形态已定：**数字错误码判别值 + 数值/枚举上下文，无字符串错误消息**；
   ts_rs 生成 TS 数字 enum；前端文本表由前端配置维护；日志人话只在边界格式化）——注意这是
   错误结构化批次的内容，本批只做了 code 载荷数值化，reason/detail 文本载荷仍待清单 B 归零。
5. 提交 C：server wire + ts_rs 再生成 + web 最小编译修复 + 配置格式升级。
6. ADR 0041 终稿补"边界规则"（含错误 wire 形态）；findings + current-state 追加；基准照测。

## 批次记录

- 批次 1（已完成）：生产代码全部 + 上述 5 个测试文件清零（1116→698）。

## 约定（二级 subagent 执行规范，机械遵循，不做设计决策）

**背景**：引擎股票身份已从 `StockCode(String)` 全面改为 `StockId(u32)`（定义 `packages/engine/src/stock.rs`，
`account.rs` re-export，`StockCode` 仍存在于 `account.rs` 但**只用于边界**）。生产代码已全部编译通过；
剩余是测试代码的类型不匹配。你只做机械替换。

**id 分配规则**：`StockCatalog` 按代码字符串**升序**分配 0..n。常见 fixture rank：
- 单代码 fixture：恒 `StockId(0)`
- ["600888","600889"] / ["600101","600102"] / ["000812","000813"] / ["600888","600999"]：前者 0 后者 1
- 五股表 ["000812","002156","300260","600101","600610"]：000812=0, 002156=1, 300260=2, 600101=3, 600610=4
- 未知/越界哨兵用 `StockId(9)` 或 `StockId(u32::MAX)`（看上下文：map 插入用 9 即可，保持测试意图）

**替换配方（对测试代码）**：
1. `StockId("代码".to_string())` / `.to_owned()` / `.into()` → 按 rank 换成 `StockId(n)`（有 `session`/`sess`/`game` 变量在作用域时优先 `session.stock_id("代码").unwrap()`）。
2. `let code = xxx.state.setup.stocks[0].code.clone();`（后续当内部键用）→ `let code = StockId(0);`（或经 `.stock_id()` 翻译）。
3. 局部 map/set 声明 `BTreeMap<StockCode,`/`BTreeSet<StockCode`/`HashSet<String>`（存代码的）→ `StockId` 相应容器。
4. `.clone()` 在 StockId 上可原样保留（Copy 类型有 clone）。
5. serde_json 断言索引 `json[..][&code.0]` → `[code.0.to_string()]`（JSON 对象键是字符串）。
6. 公司缝隙 `company_registry.issuer_of(&code)` → `state.issuer_of_stock(*code)` 或 `context.issuer_of_stock(*code)`（StockId 是 Copy；若 E0614 去掉 `*`）。
7. spec 查找 `find(|s| s.code == *code)` → 先 `let cs = xxx.stocks.code(*code).expect("...");` 再 `find(|s| s.code.0 == *cs)`。
8. `format!("{:?}", code)` 或字符串拼接断言：StockId Display/Debug 均打印数值；若断言字符串文本（如 contains("600888")）→ 改成按 rank 的数值断言或删除该文本断言并注明"数值身份后无字符串可断言"。

**不准动（红线）**：
- `StockSpec { code: ... }` 规格构造与 `stock.code = ...`/`spec.code = ...` 赋值**必须保持 StockCode 字符串**（边界）。若你看到 `stock.code = StockId(...)` 是之前误改——改回 `StockCode("代码".to_owned())`。
- company/* 与 session/company_assembly.rs 一律不碰（公司域字符串身份属后续批次）。
- 错误变体的结构/判别值、序列化字段名、id 分配序——逐处确认，拿不准就在报告中列"待人工确认"而不是猜。
- 你的清单之外的任何文件。

**禁止动作**：不跑 cargo、不跑 rustfmt、不跑任何构建/测试（编译验证由主控统一做）。只用文本编辑工具。

**报告格式**：每个文件一行 `路径 × 改动处数`，最后列出你拿不准的"待人工确认"清单（可为空）。

## 分片计划（清单在 .tmp/e8/shard-*.txt）

- 第一波（在途）：A1 session 四大测试文件 81 / B 连续撮合组 91 / C 账本校验组 75
- 第二波（待第一波验收后放）：D NPC/attention/quote_expiry 组 89 / E 拍卖/stock_stream 组 81 / F session 余量 94 / G src 库余量 71 / H 集成测试 tests/ 101
- 验收流程：git status 核对改动集只增清单文件 → cargo check --tests（-j8、299s）→ 失败修正/打回 → 更新本文件批次记录

## 批次记录（续）

- 批次 1 验收（已过）：A1/B/C 回收后 cargo check --tests 698→457；wave1 文件仅 1 处遗漏（crate::StockCode 路径，已修 + lib.rs 恢复 StockCode re-export；lib 0 错误）。B 片发现 quote_setup 规格代码被我误改 600101 → 已恢复 "600888"（保 RNG 蜡烛路径）。reorder（executor_perturbation）确认是 verification-harness 门控装置，String 身份键登记为测试装置边界例外，不改签名。
- 批次 2 在途：D（NPC 组 90）/E（拍卖组 81）/F（session 余量 92）/G（srclib 余量 71）/H（集成测试 101）。shard-*.txt 已按最新普查重新生成。

## 批次记录（续二）

- 批次 2 验收（已过）：D/E/F/G/H 回收 + 24 处漏网（examples、retail_projection 组、read_only_start/npc_state_projection 残余）主控直修——**cargo check -p engine --all-targets 0 错误**（lib+tests+examples 全部编译通过）。
- 分片确认点均采纳：quote_setup 恢复 "600888"；AuctionDayEndError 哨兵 StockId(u32::MAX)；watch 哨兵 u32::MAX-10-i；reorder（verification-harness 门控）String 键登记为测试装置边界例外。
- 下一步：cargo test --lib 定向验证（已知基线固有失败照旧不算；hash_contract 无固定 hex fixture 不需固化）→ 提交 A。

## 批次记录（终：测试验收 + 提交 A）

- **cargo check --all-targets 0 错误**；lib 1311 过/8 败；集成 11 个失败二进制经基线 worktree（dde740c5）逐一对照，**全部为基线固有**（含 company_* 两个：基线因缺 import 编译不过，补 2 行 import 后基线同样红——属被掩盖的既有失败，H 片顺带修好了 import）。
- sweep 修复过程中新增并已解决：证据十进制字符串契约（stock_id_decimal）、rank 误配（shadow tests session/standalone 双语境）、JSON 键数值化断言、symbolic intent JSON code 数值。
- 证据契约收窄登记：assert_no_json_numbers 放行 ≤2^53 安全整数（动机是 JS 安全整数，u32 id 恒安全；wire 数值是用户契约）。
- 提交 A：refactor(engine) 股票身份数值化（含 ADR 0041、清点清单、进度档）。
- 待续：清单 B（结构化错误：数字错误码 + ts_rs 数字 enum + 前端文本表边界）、C（CompanyId/venue/frozen/光标）、D（server wire + web 适配）。
