# Web 当前存档边界适配（2026-10-25）

## 真实来源与范围

- root 授权在 PlanId 机械适配之后，独立修复真实 Rust SaveSlot 触发的既有严格 parser 漂移；不改变交易规则或引擎 wire。
- 原档：`/data1/baiyifan/workplace/stock_market_game/.tmp/plan-identity-target/producer-save.json`，1819353 bytes；SHA-256：`03a7e1776a5b81e09cf6bd91b365f4eeea1229b5b4c9cf470c7653739561ca24`。
- 核心 producer 使用真实 GameSession 两日 `step → end_civil_day → save`，含 owner=3/4、sequence=0 的两项真实计划；核心执行 `decode_save_slot → GameSession::restore` 后才发布原档。Web 未改原档字节。producer 源码位于核心树 `agents/branch-completion/plan-identity-producer.rs`。
- `current-producer-fragments.json` 仅从上述原档提取完整子对象，不修改子对象字段/值；它是逐字段 TDD 的短 fixture，不替代完整原档往返见证。

## 第一批：根标记与个人身份

| 字段 | 当前 Rust 依据 | 严格 Web 约束 |
|---|---|---|
| `desk_mirrored_resting` | `session.rs::SaveSlot`、生成 `SaveSlot.ts` | 必填 boolean，原样保留 |
| `InformationState.owner`、`BeliefBook.npc` | 字段直接使用 `AccountId`，`orderbook.rs::canonical_u64_decimal` | canonical u64 string，与对应账户 key 一致 |
| 个人状态账户 map key | `AccountId` map | canonical u64 十进制键，不转 number |
| `InformationState.companies` key | 数值 `CompanyId(u32)` map | canonical u32 十进制键 |
| `BeliefEntry.company`、`CompanyMismatch.expected/report` | `CompanyId(u32)` | 数值 u32，拒绝旧代码串 |

没有默认补值、旧格式 alias 或宽松 unknown。历史大 JSON 保留原字节，仅其 test-only projection 显式投影新格式。

原始新阶段日志位于本树 `.tmp/plan-identity-web-receipts/`：

- `current-wire-red.log`：3 case 全红，exit=1；分别因未知根标记、旧数值 owner 与错误值未拒绝而失败。
- `current-wire-green.log`：实现后 17/18 绿，旧测试 fixture `company:"company"` 先于 confidence 校验失败，exit=1。
- `current-wire-green-2.log`：更新该旧测试输入为真实数值 CompanyId 后，18/18 绿，exit=0。
- `root-personal-regression.log`：59 case 绿，exit=0，wall 3.03s。

上述命令均经 `scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000`，聚焦并发4、关联回归并发8。执行 cwd 为仓库根或 `apps/web`，日志中的测试路径与命令一致。

## 类型检查与已知限制

- 已精确 cherry-pick 核心实际导出的生成类型 `21cbee78` 为本树 `0cf0abb3`，没有手改生成文件。
- 首次共享300000ms进程外编译批次并行启动 app/node/临时DEV三个 TypeScript project：app 8673ms exit=0、node 1599ms exit=0；DEV临时 config 放在 `.tmp` 造成 types 路径与 glob 配置错误，740ms exit=2，不能记为DEV类型通过。原日志在 `.tmp/plan-identity-web-typecheck/{app,node,dev}.log`。
- NPC inspector 与 EngineHost 的查询请求仍采用已有安全整数 API；本轮保证返回 PlanId 的完整 u64 与显示，不宣称请求端也支持完整 u64。
- 最初 PlanId 阶段红绿输出仅保留于会话工具 receipt，未落盘，不能补造历史原始日志：红 `090898`/`6d0d97`；绿 `a1e815`/`cf7c77`/`5e54a3`。该限制保留供审查。

## 后续批次与门禁

- CompanySpec/catalog/CompanyId、report source/scope 与公告 tagged content 仍待适配；公告六变体由 root 指派的独立 writer 负责两个新增文件。
- setup 当前模式字段需按现 Rust 定义严格适配，不冒称新局 UI 模式旅程已覆盖。
- 所有批次后须在同一未改动原档上执行完整 Web parse，并闭合 Rust restore 见证；目前未宣称此门禁通过。
- 可信独立 reviewer `sol61_pending122_contract` 逐批审查增量完整 diff；未完成终审前不宣称交付完成。
