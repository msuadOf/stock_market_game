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

## 第 19 任续（协调者批复后推进：envelope→acct→Q2）

新增三刀（61c0ea1c 起）：
- 61c0ea1c P2-3 D6-resting：观察面随 DeskReport 上行，collect 往返
  退役。B 1000 691→713.5（+3.3%）；A 配对 −2.3% 带内（desk 组装税）。
- d1ae4693 P2-4 D6-envelope 惰性化：ledger 每拍维护退役，查询/hash/
  save 派生（restore 回退分支保 hydrate 语义）。B 1000→750.3
  （+5.2%，累计 +37.7%）；envelope 段 80.5→2.7μs。
- **acct 分片刀撤回**（负结果归因）：红证=组报告时点 desk 终轮批次
  可能在途（undo∪staging 集不含终轮成交 owner——时序竞态），完整
  形态依赖 Q2 Seal（S4 不变量）。**Q2 实施设计已出**：
  plans/q2-seal-implementation-design.md（报批）。回退后双模式
  1669/0/11 双绿复核。

B 1000 进度：544.7（基线）→750.3（累计 +37.7%，高负载窗数字）——
净机复测待补；距 912 推算上限余 162μs（acct 62+candle 47+belief 安装
82+缝隙）——其中 acct/D3/D2 约 100μs 由 Q2 批回收。

事故登记：P2-4 批删死代码切坏 drive.rs（checkout+重放恢复）；P2-5
缝合残留两轮编译红——教训：regex 批删必 assert 锚+逐批编译门禁。

## Q2 Seal 四批落地（第 19 任三段，2026-10-18 晚）

| 提交 | 内容 | B 1000 |
|---|---|---|
| 74436961 | 批 2 Seal 相位（S1-S6+Start 防御+存活契约） | 750.3→747.7（持平） |
| dd1dd7b9 | 批 3 账户分片随 SealAck（D2 退役） | →751.0 |
| 5ebd0a4b | 批 4 D3 退役（unsettled 字段类型级删除） | →**782.3** |

**B 1000 累计：544.7→782.3（+43.6%，高负载窗；净机复测待补）**。
合同 8/8 绿（存活契约/unsettled 恒空→类型级/期末结算对照+突变红
Start 防御实证）。双模式 1673/0/11 双绿。

**关键事故与修复（如实）**：首版 SealAck 用 state_hash()=O(20000×5×8)
≈2ms/tick——B 掉到 202/261 实测打回→O(1) 指纹（设计稿本指定）。
eager-apply 臂（A −20.5% 护栏触发）首版破坏日结 Abort（undo 交错序
sol4 红）→撤回，undo 分域设计登记专批。

**A 口径净税（开放项）**：批 2 前 ~1174-1190 → 批 4 后 899（高负载
窗混杂）——10 组 Seal 屏障+屏障内分片构建。方向：(a) 净机配对归因；
(b) 分片构建移出 SealAck 临界路径；(c) eager-apply undo 分域版。

**Q2 后架构状态**：提交点=发布点=Seal（组账户/组簿/信封观察面在发布
版本处一致）；Echo 生产往返全退役；D2/D3 类型级删除。

**下任刀序**：①A 口径 Seal 税归因（净机配对+屏障内构建外移）②belief
安装侧 82μs（P2-2b 同款 undo 记录优化/或 belief 安装进分片消费）③
candle 47μs（派生读面——等价负担大需专项设计）④eager-apply undo
分域（A 税根治）⑤独立复核建议处置（见 reviews/）。
