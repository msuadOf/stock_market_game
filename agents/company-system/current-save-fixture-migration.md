# Current Save Fixture 迁移记录

## 范围

将 Web 的 `currentSaveFixture()` 从手工拼接状态改为读取 Engine 生成的当前档案。生成器位于
`minimal-save-fixture-generator.rs`，明确配置 `CompanySystemConfig::Simple`，以 seed 42 创建
单股 `600101` 场景，完成 2030-01-01 休市日日结后保存，并验证 `ProtocolSession::restore` 后重新保存
与原存档深度相等。当前 Engine 真实生成 account 0 玩家和 account 1 `ZiNoise/Dormant` 零售 NPC；未伪造
Momentum 运行时身份。完整 Momentum `StrategyState` 保留为独立 schema 正例。

旧顶层 `company_operations`、`groups`、`ops_wiring` 不再进入 fixture。`commandDayEndArchiveFixture()`
直接消费真实 Engine 日终档，不再修改 `settled_through`。日终候选负例改用未提交的 `seq`，存档合同测试
补入现行必填 MarketSnap 字段和 `active_minute_history`，save 命令测试显式检查恢复公司草稿。

## 验证

- 主线程使用当前 release Engine 分别编译三个正式 producer；并行 `rustc` 编译全部成功，未重复运行 Cargo。
  main 以现有完整档案提供 setup/seed，closed-day 和 minimal 各使用明确场景；三个 producer 均在
  外部 10000ms deadline 内生成 JSON，执行场景 guards，并验证 `ProtocolSession::restore` 后重新保存
  与原 JSON 深度相等。minimal producer 固定 seed 42 与显式 `Simple` setup。
- Engine 生成的 main、closed-day、minimal 档分别安装为 `current-schema-save.json`、
  `current-closed-day-save.json`、`minimal-current-save.json`。`current-company-slice.json` 是从 main 的
  `company_system` 精确投影；已比较投影与原字段深度相等。安装位置均在
  `apps/web/src/save/fixtures/`。低层 CompanyOperations 测试继续使用独立
  `company-slice-test-fixture.ts`；company slice 没有独立 Web TS consumer。
- 最终 fresh strict/deep 消费日志 `.tmp/company-system/session-actions/final-generated-three-strict.log`
  显示 `current-schema-save`、`current-closed-day-save`、`minimal-current-save` 三档全部通过
  strict parse 与深度往返。旧契约阶段的 strict 失败日志
  `.tmp/company-system/session-actions/minimal-fixture-strict-final.log` 仅作为历史证据，不能代表当前结果。
- minimal consumer 组在 10000ms deadline 下 46/46 通过，日志为
  `.tmp/company-system/session-actions/current-fixture-minimal-consumers-final.log`。ROE parser 与手工报告 fixture
  同步后，fresh Web Report/fixture 消费组 23/23 通过，包含此前失败的 `company-schema.test.ts` 三项；日志为
  `.tmp/company-system/session-actions/final-web-report-fixture-consumers.log`。Fresh app/save consumer 组
  51/51 通过，覆盖 save commands、session host、day-end candidate 与 archive，日志为
  `.tmp/company-system/session-actions/final-current-fixture-app-consumers.log`。两个结果按各自测试范围记录，
  不合并计数，避免重叠误报。此前 gold/schema 日志 `.tmp/company-system/session-actions/current-fixture-gold-schema-final.log`
  的 63/66 与缺少 `roe` 的失败已由上述更新及 fresh 23/23 验证解除；不代表完整 Web 回归。
- 此前 `company-contracts.test.ts` 的两个公开公告 parser 用例也因其独立自建 payload 缺现行必填 `event`
  失败，其余用例通过；该文件不属于本次变更范围。
- 非实施者 Luna 对三个 producer 与四份安装文件独立复核通过：restore/resave 深度相等、公司切片与完整档
  投影相等、场景日期/tick、真实 NPC 受理和 receipt cursor、金额单位及旧字段排除均符合契约。复核者未运行
  parser/test，动态结果以本节 fresh 日志为准。

## 限制

验证仅覆盖本记录列出的 producer、strict/deep 消费及定向 Web consumer；未运行完整 Web 回归或
CompanySystem 完整验收。未触碰 Git index 或提交。
