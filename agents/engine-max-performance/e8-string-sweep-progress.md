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

## 阶段二配方：结构化错误（B1 = SessionError 自由文本 321 处）

**类型**（packages/engine/src/error.rs 已建并导出）：`ValidationFault { code: u16, context: Vec<u64> }`；
ctor `ValidationFault::new(code, vec![...])` / `ValidationFault::bare(code)`；Display 只出码+数值；
serde/ts_rs 全数值。`SessionError::{InvalidSetup,InvalidSave,InvalidHistoryRead,ResourceLimit}` 的
String → ValidationFault（原子变更：321 站点一次改完才可编译）。

**站点改法**：`InvalidSetup(format!("...{a}...{b}..."))` → `InvalidSetup(ValidationFault::new(<码>, vec![<a>, <b>]))`：
- 数量/金额分/tick/分钟/id → `as u64`（i64 负值 `x.unsigned_abs()` 保幅值并注释）。
- CivilDate → `crate::error::ymd_context(y, m, d)`（`date.ymd()` 取三元组）。
- 枚举参数 → `as u64`（必要时 enum 加 #[repr(u16)]）。
- 无参数纯叙述 → `ValidationFault::bare(code)`。
- **纯文本参数（槽名/hash/代码串）**：以 0 占位 + 站点上方注释
  `// fault<code>: 槽<i> 待 C 批数值化（原文本 "<原文>"）`，并在报告"待 C 批"列表登记——禁止静默丢信息。

**码段分配**（站点顺序取号 + 站点上方注释 `// fault<code>: <一句话语义>`）：
session.rs 51 处 1000-1099；initial_allocation.rs 7 处 1100-1149；company_assembly.rs 13 处 1150-1199；
notices.rs 7 处 1200-1249；persistence.rs 196 处 2000-2199；saved_runtime.rs 26 处 2200-2299；
protocol/civil/ 21 处 3000-3099。InvalidHistoryRead/ResourceLimit 并入所在文件码段。

**测试断言改法**：`msg.contains("原文")` → 断言 `fault.code == <码>`（结构比较；首错序由哪个码先出现验证）。

**子代理禁止**：跑 cargo/rustfmt/构建；动清单外文件；改码段分配；丢弃原语义信息。

## 阶段二批次记录

- 复核修复已落 eef4f262（P0-1 escrow 双股动态 id+防回归断言；P1 candles 文档/目录重复校验）。
- B3 已完成首片（主控直做，编译 0 错误）：error.rs（ValidationFault 契约型）+ MoneyError（ParseFailReason/MoneyOp 枚举）+ OrderError（PriceReject/ProjectionMismatchKind+context）+ market/account 溢出站点 + history_reads/shared_ingress（码 1250-1260）。未提交（等 B1 编译收口后合并或分批提交）。
- B1 分片在途：S1 persistence 222 处、S2 session 装配 78 处；S3 civil 已回（码 3000-3017，两处确认点已采纳：变体各占一码、不可达兜底码登记）。
- P0-2 web 适配分片在途（摄入层翻译架构：快照/事件/历史响应数值 id→代码串，视图不动；请求侧 host 内转数值）。

- B1 收口：S1+S2+S3 全回，主控补 persistence:1146（1253）后 **lib 0 错误**；all-targets 剩 84 处测试断言，码映射分片在途（money/orderbook 构造类已主控直修）。访问器补充：ScheduledDueId::as_u64 / PublicationId::as_u32（错误上下文用）。

## 阶段二批次记录（续）

- **B1+B3 首批已落 8447ecd6**：321 处自由文本→ValidationFault（三片并行）+ money/orderbook/market 枚举载荷 + 测试码断言改写（映射分片 + 主控修 7 处边角：money display/部分移动/fee 码 2228/eviction 2119/2121/StepFatal fault1004 包装）。**lib 1312 过/8 败 = 基线一致；全部集成二进制与基线逐一对齐；money 29 全绿。**
- P0-2 web 适配：分片完成 67 处 + 2 新模块（stock-ids.ts / stock-identity.ts，摄入层翻译架构）；主控补类型别名重指（engine.ts Snapshot/Intent/Event/AccountSnap→Coded*、player-orders Event、priceHistorySlice 键、AccountSnap/WireAccountSnap readonly 放宽、save schema 3 处分号）；剩 43 处夹具类型收尾分片在途。
- 待办：web tsc 0 错后提交 web 批 → C 批（CompanyId/venue/frozen/光标 + fault 占位槽回填）→ A 表余行 → 全量基准（worker=4 口径）。

## 阶段二批次记录（终：B1+B3 已落、P0-2 收尾在途）

- **8447ecd6**（已落）：B1+B3 错误结构化（321 处 ValidationFault + money/orderbook 枚举），lib 1312/8=基线一致。
- **eef4f262**（已落）：复核 P0-1/P1。
- **在途未提交（P0-2 web 批 + server/wasm/desktop 宿主错误码）**：
  - server/desktop/wasm 宿主错误转 fault 码 1290-1314（世代过期 1290、致命停机 1291、句柄耗尽 1291/1301、锁中毒 1293/1300/1314、token 1302/1303、序列化 1312、speed 1311 to_bits）
  - routes 匹配 fault[1290] || STALE_SESSION_GENERATION: 前缀
  - ws/publisher/api_contract/auth 测试 fixture 数值 code（600101→0；setup stocks 规格位保留字符串）
  - server --tests 全绿（仅基线固有 actor_market 一例）
  - web：stock-ids.ts + stock-identity.ts 摄入翻译；IntradayPanel 补传 activeDailyCandle（均价恢复 8.67 渲染）；移动端 17/17 全绿
  - **web 剩余失败 25 处**（存档交割单/tax owner/Bank 税务/Browser intake worker/u128 字符串校验等）——多为 fixture 期望字符串键或依赖生成的数值类型，需逐个对照摄入层翻译形态修正
  - tsc -b --noEmit 0 错误（已验证）
- 下一步：修完 web 剩余 25 处 → 提交 `refactor(web): 前端契约随数值 id 迁移` + `refactor(server): 宿主错误码化` → C 批（CompanyId/venue/frozen/光标）→ 基准

## 阶段二批次记录（C 批部分 + 交接）

### 已落库
- **9bf4fe9b** `refactor(engine)`: venue/frozen 枚举化（OrderVenue/FrozenAsset；delta 校验改枚举等值）。lib 1314/8=基线一致，all-targets 0 错误。
- 顺带移除外来未跟踪测试文件（report_frequency.rs / consolidated_restatement.rs，来自 main 分支、引用本分支不存在的 API）至 `.tmp/e8/foreign-tests/`——非本任务产物，未删除。

### 在途
- **web 25 处 fixture 分片在途**（子代理运行中：存档交割单/tax owner/Bank 税务/u128 字符串/Browser intake worker）。回来后统一 `npm test` 验收。
- **未提交工作区文件**（server/wasm/desktop 宿主错误码 + web 摄入层 + IntradayPanel 修复）：web 验收通过后分两笔提交：
  1. `refactor(server)`: 宿主错误 fault 码化（1290-1314）+ routes 匹配
  2. `refactor(web)`: 前端契约随数值 id 迁移（摄入层翻译 + Coded* 类型 + IntradayPanel bug 修复）

### CompanyId 数值化设计 spec（下一棒）

**现状**：`CompanyId(pub String)` 209 处非测试引用。运行时构造点：
- 发行人：`CompanyId(format!("C-{}", stock.code.0))`（company_assembly 436/563/697）——派生自股票代码，StockId 数值化后可用 `CompanyId(stock_id)` 或独立序号
- 默认表：`id: "C-600101"` 等 5 行静态数据（defaults.rs 139+）
- 外部对手方：customer_finance 的 "issuer"/"issuer-b"/"company"/"third-company" 等自由命名（非发行人，是债权人/客户身份）
- 会计/信息/经营域：CompanyId 作为 BTreeMap 键 + 跨域引用（12-13 处/文件）

**设计要点（沿 StockId Catalog 模式）**：
1. `CompanyId(u32)` + `CompanyCatalog { metas: Vec<CompanyMeta> }`，CompanyMeta { code: String（原字符串身份，如 "C-600101"）, name: String（公司名，领域内容） }
2. 加载序：按原字符串 id 升序分配（保持 BTreeMap 迭代序 = 展示序）
3. 发行人派生：`company_catalog.issuer_id(stock_id)` 或装配时建立 StockId→CompanyId 映射
4. 自由命名的对手方：纳入同一目录（catalog 覆盖全部公司实体，不只是发行人）
5. 存档 schema：CompanyCatalog 序列化随存档（同 StockCatalog 模式），破坏性变更
6. 未知 id 显式报错测试（同 stock.rs 的 lookup_roundtrip_and_unknown）
7. **规模预估**：与 StockId sweep 相当（~200 生成站点 + 调用点机械替换），建议 3-4 分片并行
8. **注意**：company/query.rs 61 处 String 多为公司显示内容（名称/代码/金额字符串）——数值化 id 键位后这些保留为内容例外（清单 E 节）

### 光标数值化（C 批余项）
- `InvalidPublicReportCursor { cursor: String, company: String }`（information/mod.rs:91）→ 数值/枚举
- civil refresh 的 securities 键位已在 fault 占位登记（3001-3017 槽）

### 基准（阶段二完成后）
- 锚定交错配对、worker=4、占用<10% 核段
- H = 254c50b0（阶段一末）冻结 binary；C = 全部清零后 HEAD
- 合并计阶段二全部（错误结构化 + 宿主码化 + venue/frozen + CompanyId + 光标）

## 阶段二批次记录（续终）

- **2b50c6b2** `refactor(server)`：三宿主 fault 码化（1290-1314），server/wasm 测试全绿。
- **4f44063c** `refactor(web)`：前端数值 id 迁移（摄入层翻译 + IntradayPanel bug 修复）。**npm test 0 失败、tsc 0 错误**。
- 外来文件处置：main 领先的未跟踪测试/生产文件（引用本分支不存在的 API）全部移至 `.tmp/e8/foreign-web*/` 与 `.tmp/e8/foreign-tests/`，含误移后恢复的 stock-ids.ts/stock-identity.ts/生成枚举（FrozenAsset/OrderVenue 手写补齐，ts_rs 再生成时核对）。
- **P0-2 完成**：findings 未竟清单可划掉"web 适配"。
- 待续：CompanyId 数值化（spec 已写在上一节）→ 光标 → 基准（阶段二合并数字）。
