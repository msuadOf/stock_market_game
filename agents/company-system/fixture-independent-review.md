# Fixture 独立复核记录

## 范围与限制

本复核针对 `agents/remaining-questions-and-features/current-save-fixture-generator.rs`、
`agents/company-system/closed-day-fixture-generator.rs`、
`agents/company-system/fixture-migration.md`，以及
`apps/desktop/src-tauri/src/actor.rs` 中将 `simple_company.rs` 的测试 include
放入 `mod tests` 的 hunk。复核人未参与实现。本次只做静态检查，没有运行 Cargo、
测试或生成器；主 agent 正在生成实际 save，因此不预报生成成功或测试通过。

## 发现

- current-save generator 从输入 JSON 读取 setup 与 seed，解析为 `SessionSetup`，并
  经 `ProtocolSession::new`、真实 `step_frame`、`end_civil_day_update` 和 `save`
  生成输出；不是手工拼写目标 SaveSlot。它核验 5 stocks、26 NPC、seed
  `666959854`、每日 60 ticks 和 Quarterly report frequency，并要求 tick 120、
  日结状态、公司数量、Simple 模式、成员账户对应 snapshot 账户、保存恢复往返
  相等。后续还分别从原会话与恢复会话连续推进 3 frames，要求 tick/sequence
  连续且两边都实际出现 NPC `OrderAccepted`。这些断言覆盖目标真实性及续行行为。
- receipt cursor 检查只验证保存前各 ordinal 在续行保存后没有回退；未要求其增长，
  因而没有把“没有新 receipt”错误说成“发生了回退”。
- closed-day generator 通过默认 `TradingCalendar` 断言沪市
  `2026-01-01` 非交易日，再要求日结前 `civil_day_ready`，以真实 `ProtocolSession`
  结束自然日并保存。它核验零 market tick、`settled_through=2026-01-01`、
  `current_date=2026-01-02`、Simple 公司 `advanced_through=2026-01-01`，并要求
  `ProtocolSession::restore` 后重存 JSON 完全一致。这里检验的是游戏日历日结及恢复
  语义，不是用一个节假日样例证明完整 A 股节假日表。
- `fixture-migration.md` 将 69 项陈述明确拆为
  `15 + 10 + 6 + 11 + 10 + 9 + 8 = 69`，并限定为七组短测的已有记录；另将
  host64 五个 Rust 包 `--lib --no-run`、typegen 128 项、Server availability 1 项
  各自限定在其对应编译/测试范围。文档明确说本次整理未重跑这些验证，也不外推为
  fixture 迁移、Q17、完整回归或三宿主验收。数字口径与证据限制一致。
- `actor.rs` 中 include 位于 `#[cfg(test)] mod tests` 内，fixture 宏仅供测试 setup
  使用，限制了对生产模块的暴露。复核到的 diff 同时更新诊断 setup 的字段以匹配
  Simple CompanySystem。此处不涉及 A 股撮合规则变更。

## 结论

静态复核未发现上述范围内的大 A 语义漂移、无依据的交易制度主张或超出 fixture
迁移目的的复杂度。holiday 日期依赖程序实际检查的默认沪市日历，而非本文复核臆断；
将来若把该 fixture 日期解释为正式规则证据，仍须另附官方日历来源及适用日期。

## 生成文件复核补记

主 agent 报告 release66 两个 generator 均真实执行成功，current save 与 company
slice 已安装到 `apps/web/src/save/fixtures/`；Web 短测仍由 owner 待运行。独立核对
了已安装文件本身：`current-schema-save.json` 解析为 seed `666959854`、5 stocks、
NPC 计数 `12 + 10 + 4 = 26`、60 ticks/day、Quarterly、snapshot tick 120、27 个
账户；`company_system.implementation.mode` 为 `Simple`，state 中 5 家公司，且旧的
`company_operations`、`groups`、`closing_registry`、`ops_wiring` 字段均缺席。
保存日期为 `2030-01-09`，前一日已结算。`current-company-slice.json` 是一个直接
序列化的 `company_system` 对象（顶层 `implementation`、`issuers`）；将它与完整存档
的 `.company_system` 做 canonical JSON 比较，内容相同。该关系符合 slice 作为
`fresh.company_system` 的来源约定，并未额外包成 `{ "fresh": ... }`。

## Schema 修订后的最终产物复核

主 agent 报告 P1 schema 修订后 release70 两个 generator 重新编译、真实执行成功，
并安装最终文件；日志 `.tmp/checklist-wave4/host70-fixture-generation.log` 记录
“fresh70生成/恢复/后续NPC成交受理验证成功并安装”。最终 current save 复核为 seed
`666959854`、5 stocks、NPC `12 + 10 + 4 = 26`、60 ticks/day、Quarterly、tick 120、
27 accounts、Simple mode、5 companies；旧字段仍均缺席。此版每家 issuer 的
`issued_shares` 为十进制字符串，5 个 company finance 均带 `kind: Industrial`；
CompanySystem implementation state 具有 5 家公司，chart 包含现行基础科目及 5 个
Simple summary 科目：`simple_fixed_expense`、`simple_payable`、
`simple_receivable`、`simple_revenue`、`simple_variable_expense`。

release70 最终 SHA-256：完整 save 为
`eee7a14e8c622a41d0a59d6802aa2546e11ee641976bb5df28e5a10d0652d33b`；company slice
为 `2ce783a1f3cd4779d9b494c8bc0b40542b2fc673f2869d1c6b5097ac5b01ed15`。最终 slice
顶层是 `implementation`、`issuers`，与完整 save 的 `.company_system` 做 canonical
JSON 比较一致。以上 hash 替代早前 release66 hash；早前值不再代表最终 artifact。

当前 generator 源码给每个配置公司显式生成 `kind: Industrial`，其余输入来自基础
fixture 股票代码；实际生成输出符合上述跨层字段。host70 日志和 runtime restore/
3-frame 结论来自主 agent 提供的 release70 执行记录，本复核没有重新执行 generator，
也未独立验证该日志对应命令行参数。Web 短测由 owner 重复运行等待中。

本记录范围仍限于 fixture generator 的目标真实性、schema 字段与 slice 交叉一致性，
以及先前指定的 actor test include hunk；不属于 regression 审查或完整最终 diff 审查。
短测仍 pending，host64、typegen、Server availability 记录也不扩展成完整回归或三宿主
验收。

本记录的范围仍限于这两份 fixture 的真实性/交叉字段及前述 fixture 生成器和指定
actor test include hunk，不是 regression 审查。Web 短测尚未运行；host64、typegen、
Server availability 数字仍只是 `fixture-migration.md` 所记既有、限定范围的证据，不
由这次文件核对扩展为完整回归或三宿主验收。
