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

## A 归因收尾（第 19 任末段，evidence/p2/q2/a-tax-attribution.txt）

净机 3×3 实锤：A 税 −39%（1218→743）= 纯屏障 250μs + 组端构建 200μs
+ 驱动器重组 30μs（Arc 借用化已收，cf8c2a0a——A 回 774）。**优化排序
（数据）**：①eager-apply undo 分域（F1-F4 资源域即时/F5-F6 计划域延
迟——根治屏障+构建外移，预期 A 恢复 1150+；首版教训见批 2 证据）
②组端 dirty set/拷贝减量（与①重叠）③G 线性面靠①支撑。
B 1000 净机 749.4（+37.6% 累计；best 803）。

## 第 20 任：eager undo 分域落地+candle 报批+belief 刀 a（2026-10-19）

| 提交 | 内容 | 实测 |
|---|---|---|
| a1a4f02d | 批 1 合同红先行（apply_batch_immediate 现状桩+可交换性守卫；红证 evidence/p2/eager/batch1-red.txt） | 合同 2 红/合同 3 绿（数学守卫）如实登记 |
| 2a7e704b | 批 2 分域落地（F1/F2/F3 即时域进等待窗+staging 退役+account_dirty_set 免重扫+seal_acked_len 日结守卫+batches_after_seal Start 防御） | 双模式 1675/0/11 ×2；突变红双证（旋钮①延迟域即时化→sol4 红/旋钮②漏 Cash undo→合同 2 红） |
| faf60c6d | candle 47μs 退役设计报批稿（plans/candle-install-retirement-design.md） | **等协调者批，未实施** |
| 7c52a392 | belief 刀 a：安装链 collect 层退役（发布版本借用迭代+cache move 序） | belief_echo 段 82→72.8μs；端到端 +0.2% 噪声带内如实登记 |

**正式数字（静置窗 load 1min 3.7-4.0/5min 4.8-5.0/15min 5.5，2026-10-19 深夜）**：
- A 3000（G=10）：**912.7 mean**（882.0/925.3/930.7）——vs Arc 优化 774.7
  **+17.8%**；vs preQ2 1218 仍 −25%。Seal 相位 19-20%（206-217μs 稳定）。
- B 1000：**747.3 mean**（775.2/694.7/772.1）——vs 749.4 持平（694.7 为
  静置窗内低轮，按纪律不剔除）。
- 同窗配对锚（高负载窗）：eager 913.1 vs arcopt 818.3（+11.6%），
  Seal 相位 406→215μs（−47%）——机制归因实锤。

**A 未达推算 1120 的归因（负结果纪律，已闭合）**：推算"屏障 250μs→~0"
不成立——分域消除的是组端串行应用部分（406→215μs），残余 215μs 是
**协议链固有地板**（10 组 Seal 广播+组唤醒+延迟相+分片 clone+Ack 回传
——wall 差对账：821μs（preQ2）+215（Seal）+~60（其他）≈1095μs 实测
闭合）。进一步压缩需协议级改动（Seal 换向与下一 Start 合并）或组端
账户 COW（设计 §4 已登记不阻塞）。

**下任刀序建议**：①candle 设计批复后实施（47→~5μs，B +2.9% 推算）
②belief 深刀评估（72.8μs 剩余=cache clone+session 内部——条目 Arc 化
随 candle 批协议面同开）③Seal 相位地板的协议级方案（A 恢复 1100+ 的
唯一路径——换向合并/COW 需新设计报批）④复核登记项继承（组端 Seal
Failed 注入测试/corrupt_sparse_echo_omit_last 清理/GroupDayDone O(N)
tpd=1 边缘/首拍 force_full）。

**本任复核**：fork 独立复核已发（四提交），结果见 reviews/（归来后
补录）。事故与教训：无（regex 批删改用 python 括号配对+逐轮编译门禁）。

## 第 20 任收尾（复核+登记项清理）

- **独立复核**（reviews/independent-review-r20.txt）：**无阻断**。2 条
  登记级建议：①迟到 AcceptedBatch 的 undo 在日结转正后落入保留段=
  日结 Abort 现在会冲回迟到受理（原版不可冲回——**保守方向语义决策**，
  正常路径无观察面，特此登记）；②handle_start 防御性 apply_deferred
  冗余留作纵深。
- **登记项清理**（fb869ede）：死旋钮 corrupt_sparse_echo_omit_last
  四件面类型级删除（P2-4a 红证测试随 Q2 批 3 退役后零消费者）。
- wasm check 通过；双模式 1675/0/11 ×2（本任全程）。

**继承登记项（第 21 任）**：组端 Seal Failed 注入测试；GroupDayDone
O(N) 哈希 tpd=1 边缘；首拍 force_full 保守面。

## 第 20 任续 2：candle 批复落地+登记项收口（2026-10-19 深夜续）

| 提交 | 内容 | 实测 |
|---|---|---|
| d83560fd | candle 批 1 合同红先行（4 等价锚+2 旋钮桩，evidence/p2/candle/） | 4 绿基线+2 旋钮红（注入面红先行） |
| 3991650f | 批 2 刀 1+2（StockPublic 扩 2 字段+install 读发布版本+Echo 往返退役+循环瘦身；J6 扩面消盲区；sol3 sever 语义改写） | **candle_install 段 41.13→0.335μs（−99.2%）**；合同 6/6 绿+负控双红 |
| d5a41693 | 证据补录（B 静置窗配对） | b-bel mean 759.0 vs b-candle 5 轮 mean 770.5（+1.5%，双峰 700-841 方差如实记；与 +2.8% 推算同向） |
| a01e33ed | **登记项收口**：组端 Seal 协议错位防御（注入测试发现缺口——两分支只 break 不上报，补 Failed+abort）+复核归档 | 双注入测试绿；A 抽查 922.1 无回归 |

**复核**（reviews/independent-review-candle.txt）：无阻断。2 登记级：
①"Seal 后 desk 静止"是发布时点等价的协议前置条件——**若未来协议允许
Seal 后 desk 演进（异步盘后处理类）会分叉且无运行时守卫**，登记；
②缩进（已修）。双模式 1683/0/11 ×2；wasm 过。

**B 口径累计对照**（基线 544.7→现在）：eager 分域 747.3 → candle 刀后
770.5（配对 mean；累计 **+41.5%**）。candle 段退役后驱动器残余镜像税：
belief 72.8 / retail 19 / acct_writeback 12 / acct_echo_collect 11。

**下任刀序**：①Seal 相位地板协议级设计（A 恢复 1100+ 唯一路径——
换向合并/COW，需新报批）②belief 深刀（72.8μs=cache clone+session
内部——Arc 化条目协议面或容器整态化，负担递增评估）③剩余登记项
（GroupDayDone O(N) tpd=1 边缘/首拍 force_full 保守面/复核前置条件①）。

## 第 20 任续 3：Seal 地板分解+验证批（2026-10-19 夜末）

| 提交 | 内容 | 数据 |
|---|---|---|
| 198fd2d1 | Seal 地板插桩+攻顶设计报批（归因修正：长尾非协议固有） | 链路 17-18μs；spread 219-261μs；组端 2/19/154-156μs（G=10）、4/42/191（G=5） |
| 0106b712 | 验证批：H1/H2 排除+H3 实锤——设计稿 §3 更新选刀 A-1' | 组 0（唯一活跃组）handle 241μs=分片 226μs（7-Vec 深拷贝×活跃集）；drain 恒 0（等待窗消化完全——分域按设计工作）；闲组 2.5-7.9μs |

**A 主线新推算**：刀 A-1'（GroupAccount 7 Vec 域 SmallVec 内联化——
现有全部 fixture 股数 ≤5 恒内联，clone=栈拷贝零堆分配；零依赖手写
优先，smallvec 需 ADR）——H3 226μs 全消 ⇒ 长尾组 241→~15μs ⇒
**A 890-913→~1090-1180，单刀触 1100 线**。等协调者批复
plans/seal-floor-decomposition-design.md（数据节已按验证批更新）。

第 21 任接手面：①Seal 攻顶批（批后按 §3 刀 A-1'）②belief 深刀
（72.8μs）③剩余登记项+复核前置条件（"Seal 后 desk 静止"协议前置）。

## 第 20 任续 4：Seal 攻顶 A-1' 负结果→A-1 落地（2026-10-19 夜极）

| 提交 | 内容 | 实测 |
|---|---|---|
| b821e19f | 攻顶批 1：InlineVec 类型（容量 8=P100×2 幂上取）+分片等价锚 3 条+旋钮桩双红 | 合同 3 绿锚+旋钮红先行 |
| 5f9908db | **A-1' 负结果归因+A-1 Arc COW 落地**（accounts=Vec<Arc>；写点 34 处 make_mut；分片条目 Arc 共享；决策签名 &[&GroupAccount]） | **组端 Seal 处理 max 154→33-37μs（−78%）**；Seal 相位 206-217→**139-152μs（−30%）**；B 配对 798.8→814.0（+1.9%） |
| 799a6bf3 | 复核归档（无阻断+3 登记级） | — |

**A-1' 负结果**（如实归因 evidence/p2/sealtop/batch2-arc-cow.txt）：内联化只消分配开销——组 0（唯一活跃组 2000 NPC 全 dirty）分片成本主项=**搬运量**（2000×450B/拍）；enum 膨胀反损缓存。条款触发转 A-1 ✓。
InlineVec 保留（闲组微降+COW clone 栈拷贝复用+spill 合同）。

**A 速度**：3 轮 mean 866.6（15min load 9+ 热窗——**正式数字待静置窗补**，
批 2 正式 912.7 对照在 15min 5.5 窗取）。结构性指标（Seal 相位/组端处理/
B 配对）不受负载影响已确凿。**1100 线预期**：Seal 相位 139→静置窗若
spread 余量同步收敛（调度税与负载相关），A 推算 ~950-1050；触 1100 还需
spread 的调度余量（~100μs）压缩——刀 C（taskset 亲和）或后续流水线
（B-1）候选，数据驱动再定。

**复核登记**：ensure_fact_owners_covered_arc 每拍值拷贝（下游只用
local——driver 侧优化面）；InlineVec pub 变体可见性（低风险）。

**第 21 任**：①静置窗补 A 正式数字（s5-arc 二进制 .tmp/eager/）②若
spread 仍 >100μs：刀 C taskset 复测（零代码）③ensure_fact 优化面
④剩余登记项。

**刀 C 定性复测（182de608，收尾指令）**：绑核=方差收紧器（±85→±26）非
spread 解（137→136——真源=消息链级联+load 扰动）；热窗减弱后 A 单轮
**1057.9**/mean 963——静置窗正式预期 1000-1100+。第 21 任指令模板
（静置条件+命令+基线+判读）在 evidence/p2/sealtop/knife-c-taskset.txt。
本任停手。

## 第 21 任：登记项收口（2026-10-19 凌晨）

七条登记项逐条处置（1 落地 / 6 维持登记——每条附结论依据）：

| 登记项 | 处置 | 依据 |
|---|---|---|
| ensure_fact_owners_covered_arc 每拍值拷贝（sealtop 复核） | **落地 b271f9bd** | 覆盖校验核心抽 local 下标单源（ensure_fact_owners_covered_locals），Arc/值形态薄包装共享；判别面 p2_4a_arc_shard_cover_check_matches_value_form（正/负/None+与值形态错误逐字等价），突变红实证（短路版负例红）；双模式 1689/0/11 |
| InlineVec pub 变体可见性（sealtop 复核） | 维持登记 | 收口牵连 GroupAccount→UpMsg→decision pub fn 整条 pub 可达链（runtime mod 必须保持 pub——apps/server 消费 step_runtime_enabled）；crate 外零消费者，无现实风险面。触发条件：engine 出现外部库消费时一并收 |
| GroupDayDone state_hash O(N) tpd=1 边缘（Q2 复核） | 维持登记 | state_hash=Abort 回滚等价判据（正确性承载面非纯诊断）；增量哈希需 34 写点+undo 逆更新，断链即静默放行漂移（违背铁律 2 精神）；tpd=1 边缘形态现役口径无触发，单次日结 O(N)≈几十 μs 占比 <5% |
| 首拍 force_full 全量往返（Q2 复核） | 维持登记 | 正确性必需：构造后 account_dirty_set 为空，首拍读 Seal 分片=装空镜像；全量往返=一次性成本（非每拍税），保守面无害 |
| "Seal 后 desk 静止"前置无运行时守卫（candle 复核） | 维持登记 | 结构性保证已存在：DeskIn 仅 Place/CancelAll/Cancel——desk 纯输入驱动，Seal 后协调者无输入面即无演进；加守卫需给 desk 引入 Seal 感知（协议面改动）为不存在路径付税。未来协议引入 Seal 后 desk 演进（异步盘后处理类）时此登记升格为必做守卫项 |
| 迟到 AcceptedBatch 日结转正语义（r20 复核） | 维持登记 | 保守方向语义决策已定（日结 Abort 冲回迟到受理）；正常路径无观察面 |
| handle_start apply_deferred 冗余（r20 复核） | 维持登记 | 纵深防御保留（显式意图） |

全量回归过程如实记：debug 首轮 1 failed（单发 flake 复跑绿）；release
中间轮 npc_decisions_tests 家族 3 failed（既有登记的 npc rayon flake——
隔离复跑 19/19 绿+全量复跑 1689/0/11 绿，高负载窗跨测试污染，不批改）。

**belief 深刀评估（第 21 任，不建议本任实施）**：残余 72.8μs 构成=
每 dirty 账户 7 域 clone（watchlist/price_memory/information/belief/
attention/history_reads/retail——set_runtime_personal_state 内 6 次+
belief_cache 1 次）+4 次 undo 记录+3 容器 insert。刀 a 已收中间层。
深刀两条路均大手术：①七域 Arc 化——BeliefBook 等是 crate 公共值语义
类型，Arc 化破坏公共 API 面，牵动 BeliefParticipantState/session
容器/读面全链；②容器整态化——session 三容器（belief_participants/
npc_attention/history_reads）合并重构。收益上限 ~5%（72.8/1447μs）
且只影响 B（A 探针直驱不经 belief 安装）。A 主线（1000 线）收官后
按需再议。
