# Sol 三审补证：451dbfaa 双重红验证的可复验红态产物（2026-10-15）

复现方法：临时破坏生产面（每份日志对应的破坏点见下）→ 跑对应测试
--exact --nocapture 存红日志 → 恢复生产面（git diff 干净后跑两轮
lib 全量确认绿态）。破坏均在提交 54c42c14 绿态基线上施加。

| 日志 | 生产面破坏点 | 红断言（日志内原文） |
|---|---|---|
| t1-frozen-red.log | group.rs handle_day_end ② 解冻前 `if true { return Ok(()); }` | "日结解冻：frozen_today 全零 left: 5600 right: 0" |
| turnover-red.log | desk.rs rollback 的 Market 快照恢复跳过 turnover_today | "turnover 回滚后恰一轮成交额 left: 200000 right: 100000" |
| dup-accept-red.log | event_bridge.rs project_lifecycle_events 重复投影首条 Accepted（batch3+batch4 两测试） | "受理 id 不得重复：(3,0,2)" / "(2,0,2)" |
| lib-run1.log | 无（绿态） | 1558 passed / 0 failed / 11 ignored |
| lib-run2.log | 无（绿态，连跑第二轮） | 1558 passed / 0 failed / 11 ignored |
