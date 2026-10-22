# P0 缺陷：组端 root 读上下文账户簿漏加 roster_offset（多组运行时 panic）

> 2026-10-22 发现于 B4c 性能探针准备过程。**main 预存缺陷，非 B4c 引入**。

## 现象

`ENGINE_STEP_RUNTIME=1` 且 `n_groups ≥ 2` 时，尾组线程（实测恒为
`rt-group-15` 等尾部组）在决策链到期拍 panic：

```
thread 'rt-group-N' panicked at packages/engine/src/session/decision_chain/roots.rs:276:55:
account ID is absent
```

单组（生产缺省 `from_session` → `n_groups=1`）永不出事 ⇒ 长期未暴露。

## 根因（诊断 agent 逐行核验，协调者抽查确认）

`packages/engine/src/runtime/group.rs:4787`（`build_root_read_context`）：

```rust
let Some(&account_id) = projection.account_ids.get(local) else { ... };
```

`projection.account_ids` 是**全局**名册投影，正确坐标应为
`self.roster_offset + local`——同文件其余 6 处读取点（1705/1759/1816/2374/
2727/3798）全部如此，唯独 4787 用裸 `local`。组 g≥1 因此把**组 0 的账户 id**
装进 `AccountBook`，`run_chain_decision`（group.rs:2727 正确解析出
`roster_offset+local` 的全局 id）在 roots.rs:276 查不到 ⇒ panic。
组 0 `roster_offset==0` 恒正确，掩盖缺陷。

诊断另指 `group.rs:1565`（`fill.owner.1`）属同类漏偏移，待复核确认影响面。

## 排除项（诊断实测证伪的猜测）

- **padding 是红鲱鱼**：`groups=2`（20008/2 整除）、`groups=4`（5002）均 panic。
- **触发时点随机但绝对**：取决于尾组链位最早 attention `next_due`
  （session.rs:3295 逐 NPC 采样）；实测阈值与 ticks_per_day 无关——
  tpd=15300/g=16 → 75 拍 OK、80 拍 PANIC；tpd=30/g=16 → 20 拍 PANIC。

## 影响

1. **多组性能剖面全部不可测**（探针实测只能 `groups=1`）；
2. 未来任何把生产切到多组的计划在此缺陷修复前不可行；
3. B4c 性能探针的"规模敏感性（多组）"维度被阻塞——单组两规模点仍可测。

## 修复方案（诊断建议，待实现批执行）

给 `build_root_read_context` 增加/使用 `roster_offset`：4787 改
`account_ids.get(self.roster_offset + local)`；同函数内
`cash_with_pending_at`/`held_qty_with_pending_at` 两个取数点同式核查
（当前也传裸 `local`）；`fill.owner.1`（1565）同批确认。
**TDD**：先写"双组 + 尾组链位到期"的最小红测试（短 fixture，10s 门内），
再修，再全量 + 性能对照（多组恢复可测）。

## 状态

- [ ] 红测试（双组最小复现）
- [ ] 修复 4787 + 两处伴生取数点
- [ ] 1565 影响面确认
- [ ] 多组探针恢复（补 B4c 规模敏感性维度）
