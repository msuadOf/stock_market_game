# 视图内生战役台账（第 19 任交接，2026-10-18）

> worktree `.worktree/view-campaign`，分支 exp/view-internalization。
> 主线方向："裸引擎疯狂跑，分布式维护 view，外部直接观测，零会话层税"。

## 本任完成（main..HEAD 四提交）

| 提交 | 内容 | 实测 |
|---|---|---|
| e4e4c6fb | P0 分段画像探针（step_profile 24 段插桩+STEP_PROFILE=1）+基线复测落盘 | B 300=307.3 / B 1000=544.7 / A 3000=1332.6（本树锚：events 2105/6016、守恒 6701200/651403179/6668400） |
| d9cfa655 | P1 发布协议（publication.rs：PublishedTick+ViewPublication RwLock 先行版；install_tick 单点 publish；J1-J6 过） | J2 load p50=30ns/p99=2.3μs；发布开销不可见 |
| 3bda2835 | P2-1 D6 attention heap 每拍全名册重建退役（惰性化+hash 派生） | B 300→339.2（+10.4%）、B 1000→640.6（+17.6%）、attention_heap 段 258→23μs |
| 4474fc25 | P2-2 D1 belief Echo 独立往返退役（稀疏分片随 GroupReport 上行） | B 300→350.1（+13.9% 累计）、B 1000→691.0（+26.8% 累计）、belief_collect 段归零 |

全量双模式回归：1667/0/11 ×2（两次既有 flake 家族随机出现已登记：
panic_transport / npc_completion_channel rayon 域——隔离复跑均绿，
main 对照绿）。

## P0 画像核心结论（evidence/p0/p0-conclusions.md）

六项可删（D1-D6）在 1000 tick 稳态 = **≈795μs/tick = wall 的 43%**
（attention heap 14%/belief echo 12%/envelope 8% 三大件）。step() 内
未段化缝隙 319μs（内联代码+段间胶水，开放项）；wall−step_outer≈331μs
含冷启动摊薄+调度。理论 B 1000 上限（六项全删+零新增税）≈912 t/s；
叠加缝隙回收才触 1000+。

## 当前分段画像（P2-2 后 1000 tick，wall 1447μs/tick）

world_run 357 / envelope 139 / belief_echo 82（安装侧残余）/
acct_echo_collect 62 / candle_install 47 / retail_diag 19 /
acct_writeback 17 / plans_echo 17 / desk_mirror 16 / market_install 净 5 /
其余微小。

## 下一任刀序建议（数据说话）

1. **envelope 139μs（D6）**：最大剩余件。方案：DeskReport 增 resting
   观察面字段→PublishedTick→EnvelopeLedger 惰性派生（hash/save/查询
   时重建——business_state_hash 仅测试调用已证实）。坑：&self hash
   约束、undo/回滚面（回滚后重新 collect=拍前 facts 可行）。
2. **candle_install 47μs（D4）**：PublishedTick.desks[].view 已含
   daily_bars/daily_active——派生读面（等价证明负担大，设计文档 (c)
   的警告适用）。
3. **acct_echo_collect 62μs（D1 残余）**：账户稀疏往返——同 belief
   式随 GroupReport 上行可砍往返（组端收尾构建账户稀疏分片）。
4. **D2/D3（双写/同拍投影）**：依赖 Q2 Seal 收尾确认相位（设计稿
   view-design-commit-ingress.md §1.2 的 S1-S6）——大协议刀，先行
   红测试（#103 语义单测+Continue 恰一次）。

## 发布协议状态（P1 边界如实登记）

- PublishedTick v1：tick/global/desks/groups(belief)/events(占位)/hashes。
- events 段占位（事件迁移时填充——R4 语义/seq 不变）。
- 存储槽 RwLock；J2 数据不支持升级 ArcSwap。
- 组分片现仅 belief 稀疏观察面（账户投影/计划进度待接入）。

## 开放项（P0 遗留）

1. step() 内缝隙 319μs/tick 逐语句归因（内联展开面；dwarf 断栈+
   LBR 不可用，建议 frame-pointer 重编或 samply）。
2. 主线程决策链符号归属（v2 叶统计 ~5-8%——疑内联帧误归属）。
3. B 探针 fee sink 对账欠账（fees_paid 恒 0 断言边界）。
4. A 口径受分片构建影响：P2-2 后 A=1308（-1.8% 带内）+一次 load 15
   窗口 1280/1202（只登记不归因）——净机复测待补。

## 纪律备忘

- 测量：净机/静置/±15% 噪声带/≥3 轮 mean-min-max（本任全部遵守）。
- 一件一提交+显式 pathspec；突变红逐刀实证（sol5/d3b5 承载）。
- 独立复核：第 19 任末已发 fork 复核（结果见 agents 或交接记录）。

## 独立复核（第 19 任收尾）

无阻断。4 条非阻断建议：①wasm check（已闭合：通过）；②旧管线 hash
双源对照守卫（登记）；③runtime_heap_undo 死字段清理（登记）；④跨
Continue 多轮注入守卫（登记，经 ForceContinuationForTest 基建）。
详见 reviews/independent-review-r19.txt。

A 口径收尾：load 15-17 窗口 1280-1319（只登记）；分片构建成本对 A 的
净机净效应待下任静置复测（预期 −2% 内=组端每轮 dirty∪pending 组装）。
