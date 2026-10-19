# B4a 批合并执行记录（main ← feat/reintegration-b4a-company-domain）

> 合并执行 agent 产出台账。分支 tip `8f82794f`（基 `fe9e2ff5`），并入 main `a6911c52`。
> 实现批台账见同目录 [`reint-b4a-record.md`](reint-b4a-record.md)；本文件记录合并侧
> 冲突解决、跨批语义适配、验证数字与声明复验结论。

## 合并与冲突

- `git merge --no-commit` 冲突 **1 个文件**：`packages/engine/src/orderbook.rs`
  （双侧在同一位置各自新增函数体逐字相同的 `canonical_u128_decimal`，B4a 侧另增
  `canonical_u64_decimal` + AccountId serde 切换）。解决=真双改并集：保留一份
  `canonical_u128_decimal`（注释合并"B2 补面 18725d07 基线/日内均价 wire 面"与
  "公司域存档用"双用途说明），`canonical_u64_decimal` 与 AccountId 切换取分支侧。
- **双向校验双 PASS**：合并结果 vs B4a tip 的差异文件集 ≡ main 侧自基点改动集
  （68 文件）；合并结果 vs main HEAD 的差异文件集 ≡ B4a 改动集（247 文件）。
  即 B4a 全量落地 + main 侧 W3/B2fix 改动全量保留。
- 主 checkout 乱源 web 生成类型（5 改+63 未跟踪）经逐字节比对与 B4a tip 完全一致
  （零独有信息）后清理，未入提交。

## 跨批语义适配（合并暴露、非文本冲突）

### 1. web AccountId string 化波及（B4a × W3/web 语义冲突）

> 实际适配规模在首轮 tsc 收口后继续扩大（运行时面 tsc 不可见），最终
> 共 21 文件：解析器 2（orders.ts/parse.ts）、生产判断 3（App.tsx/
> player-orders.ts/effects.ts）、wire fixture 构造器与稳定键 4
> （protocol-test-fixtures.ts 的 account/entity.Account、runtime-delta、
> protocol-parse 等）、save/公报校验器同步 1（public-report-normalize.ts
> 按 B4a 契约补 source/roe/report_period 三面+摘要对账）、金样与测试件 11。

B4a 将 AccountId 全局切 canonical 十进制字符串（ts `number`→`string`）。B4a 基点
无 W3 web 代码故其自报"web tsc 0 错误"；合并后 W3 web 消费面（生产 4 文件+测试
10 文件+存档 schema）编译红 64 处。适配 15 文件（工作区 diff，随合并提交）：

- `save/schema/orders.ts`：新增 `accountId` 解析助手（复用 `decimal`——canonical
  u64 十进制串），4 处 `integer()` 换用（resting/filled owner、lifecycle account、
  pending receipt owner）；OrderId/seq 保持 `integer`（Rust 侧仍 js_safe，有意）。
- `host/protocol/parse.ts`：7 处 AccountId 面（Trade maker/taker、四事件 account、
  EntityRef::Account）`safeInteger`→`parseDecimalId`（既有 canonical 串解析器）。
- `App.tsx`/`components/player-orders.ts`/`host/protocol/effects.ts`：玩家账户判断
  `=== 0`/`!== 0` → `=== "0"`/`!== "0"`（canonical 零形态）。
- 10 个测试件 AccountId 字面量数值→字符串（含 protocol-effects.test 未被 tsc 锚定
  的字面量，防 `0 !== "0"` 运行时静默翻转）。
- `public-report-fixture.ts`：补 B4a 报表新增必填 3 字段
  （`report_period_net_income`、`report_period`、`report_period_net_income_to_parent`）。

### 2. desk 占位 K 锚定泄漏修复（main 预存回归，合并面暴露后修复）

- 现象：`protocol_civil::civil_replay_is_ordered_atomic…` 确定性失败
  （"股 1 分钟 OHLC 与日K不一致"，`retained_freeze` 不变量）。
- **三方对照实证**：pristine main `a6911c52` 同错失败（W3 desk 重写引入）；
  B4a tip（旧管线）通过；merged 失败=继承 main 侧。
- 机制：`runtime/desk.rs` `finish_round`/空簿开盘清算预置 volume=0 占位 K
  （OHL=锚定最新价），后续真实成交 `get_or_insert` 并入占位 K 继承锚定 open/low
  →成交日日 K open=昨收/初始价而非首笔成交价，与纯成交口径的 B2 分钟 bars 失配。
- 修复：`desk.rs` 成交合并点——`candle.volume == 0`（占位形态）时首笔真实成交
  重立 OHL=成交价；全日无成交标记形态（volume 恒 0）不动。
- 独立复核确认：`session/candles.rs:163-172` 与
  `pipeline/incremental_continuous_stock_shadow.rs:820-833` 本就有同款
  "首笔成交丢弃占位 OHLC"语义，desk.rs 是三处唯一缺失点——修复=对齐既有语义。
- 登记债：无成交日 K 以昨收标记的口径未入 `docs/trading-rules.md`（建议补行）。

## 验证数字（合并态，机器 128 核）

| 面 | 结果 |
|---|---|
| engine lib 默认（std-thread） | **2202 passed / 0 failed** / 11 ignored（desk 修复前后两轮一致） |
| engine tests 集成面（--no-fail-fast 全量） | **3381 passed / 8 failed**：1=desk r09 满载偶发（隔离 3/3 过，台账既有登记）；7=双亲预存（save_contract×5+player_snapshot 三方同红、full_day 双亲分层差异）——**合并引入 0，另修复 civil_replay（main 预存红→绿）** |
| serial 逃生门（ENGINE_STEP_RUNTIME=0）lib | **2201 passed / 1 failed**——memberships 既有失败，与 B4a 台账基线登记完全一致 |
| wasm32 编译 | engine 与 web-wasm 双包 `cargo check` 通过 |
| web tsc（清 node_modules/.tmp 真实缓存全量） | **32 错（31 app+1 node），位置集合与 pristine main 完全一致（合并新增 0）**；注意 tsbuildinfo 实际位于 node_modules/.tmp，清根目录无效 |
| web 单测（run-web-tests.mjs） | **1022 通过 / 0 失败**（pristine main 基线 800 通过/3 失败——合并+B4a 契约适配净增 222 通过） |

## 声明复验（B4a 台账"基线 15 失败全部转绿"）

如实记录：**该声明不可完整复现**。

- `civil_replay`：B4a tip 确实绿（旧管线），main 红（desk 回归）；合并态经 §2 修复
  转绿——"转绿"成立但归因应为"B4a 树走旧 desk 管线"，非"main 校验链顺带修复"。
- `save_contract`×5（tampered_calendar_policy、duplicate_operating_due_id、
  zero_and_overflowing_plan_horizons、invalid_bank_ecl_with_company_context、
  invalid_industrial_loan_facts）：**pristine main、B4a tip、merged 三方同样 5 失败**
  ——B4a 台账"转绿"声明在其自身 tip 上不成立（疑其全量跑被 cargo fail-fast 截断
  或复用了过期工件）。该 5 失败属 save/restore 校验链域=B4b 既定范围，非本合并
  引入，留 B4b 处置。
- 其余（step_skeleton 锚×3、player_snapshot、full_day 等）：合并态全量 no-fail-fast
  结果见测试日志；step_skeleton 锚重钉依据（AccountId 存档字节+公司结构）经查
  diff 属实（旧锚注释保留，非弱化）。

## 独立复核（fork 两轮，未实施改动方）

- 第一轮三专项：①AccountId 全局切换**通过**（BTreeMap 键天然字符串，局部 with
  必致跨层双形态；Rust 侧零双形态残留；server/wasm 生产面无数值假设）②域模块
  架构纯度**通过**（抽验 share_split/accounting/information 零 session/runtime
  引用；runtime/ 零改动）③拆股域测试真实性**通过**（断言具体数值与不变量，
  非同义反复；除权公式按沪/深交易所规则分列，银行家舍入标注简化）。
- 第二轮合并适配：desk 修复/web 适配/orderbook 并集**全过**，零阻断。
- 登记债两条：`serde-normalize.ts:70 prepareSaveForWasm` 死码按 js_safe 旧约定
  转 Number 键（无生产调用，建议清理）；trading-rules.md 补无成交日 K 口径。

## main 预存债清单（非本合并引入，需后续批收口）

1. **web tsc 32 错**（W3 批遗留）：W3 web 层把 company_id 当 string 贯通
   （coordinator/fixture/store，生产+测试），而生成面 `company_id: number`。
   W3 批"tsc 0 错误"声明疑受增量 tsbuildinfo 缓存掩盖（清缓存即红）。
   修复需 W3/B4f 侧定夺 string↔number 边界，本合并不越权处理。
2. **save_contract 5 失败**（双亲共有，见上）。
3. 桌面 `day_high`/`day_low` 字段的昨收锚定（`DeskMarket`，观察面口径）与本修复
   的 candle 口径不同源，锚定泄漏对观察面是否存在同款问题未查（W3 域）。

## 遗留交接

- 合并提交范围：merge 冲突解决 + §1 web 适配 15 文件 + §2 desk 修复 + 本台账。
  memory/ 目录用户文件不入提交。
- 临时物：`.tmp/main-pristine`（对照用 detached worktree，含软链 node_modules）、
  `.tmp/*-tsc.log`、`.tmp/b4a-merge-*`——验证完成后可清理。
