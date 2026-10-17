# P2-4b：信封账本增量维护——设计与证据

日期 2026-10-17。提交 244ae360。

## 动机（P2-4a 后画像 p14-fp2-after-4a）

- LedgerPagedMap::insert 4.77%（三单体化=EnvelopeLedger::new 三树）+
  drop_in_place<EnvelopeLedger> 4.13%（每拍整替丢弃）——信封段 ≈9%。
- resting 存量 305(t50)→4766(t1000)：全量重建成本随时长线性放大
  （1000 tick 比 300 tick 重 ~2.5×）。
- 协调者批文约束：hash 等价守卫（同输入全量重建 vs 增量逐位一致）+
  消费面逐一盘点。消费面盘点结论：runtime 路径 ledger 只被
  **内容读面**消费（business_state_hash/save/restore/玩家在途投影
  /EnvelopeLedger::eq——全部键序内容语义）；唯二按对象身份失效的
  派生缓存（reservation Weak 页身份/evidence）以显式清空对齐全量
  重建的隐式失效语义，且 Rust Arc::make_mut 在 Weak 持有下强制克隆
  （指针变即失效）双保险。terminal/seen_local_keys 在 runtime 路径
  无写入方恒空=与全量重建逐位一致；next_receipt_index 每拍显式同步。

## 机制

- 驱动器 `resting_echo_prev` 基线 + `resting_delta_since_last_echo`
  差分（(Owner, Side, order_serial) HashMap 键控 O(n)；Added/Removed/
  Changed 三分类；基线缺失/股面错配/红证旋钮退 full_rebuild）。
- EnvelopeLedger 增量方法：upsert（validate+三树写，conservation
  非 EMPTY 显式拒绝）/remove（缺席键显式拒绝）/receipt 序号同步/
  派生缓存清空。
- failure.rs：全量重建与增量 upsert 共用 runtime_resting_envelope
  构造函数（数值面同源=等价由构造保证）；名册外 owner 两路径同语义
  跳过。
- Side 补 Hash derive（差分键）。

## 守卫与突变

见 p2-4b-green.txt（M1 跳过 Changed/M2 跳过 Removed 均哈希红；
判别力建设三次迭代如实登记）。

## 数字

B 口径 300 tick 263.1→309.1（+17.5%）；1000 tick 271.6→489.7
（+80.3%，长跑反超——resting 增长税消除）。events/fees 逐位保持。
