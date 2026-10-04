# ADR-0008: GPU 与计算 offload 技术分析（结论与决策）

- **状态 (Status):** accepted（`evolve_v`/VParams 接口示例已随共同 V 删除而移除——2026-09-11 任务 26 以公司信息 + 个体判断替代隐藏 V 轨道；`ComputeBackend` 的纯批量 decide 契约保留）
- **日期 (Date):** 2026-06-30
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** 细化 [ADR-0007](0007-three-deployment-frontend-framework.md) §6（GPU seam 预留）；依赖 [ADR-0005](0005-unified-engine-three-deployments.md)（tick 步进 / 种子化 RNG）、[ADR-0006](0006-npc-strategy-module.md)（Strategy trait）、[ADR-0002](0002-engine-rust-wasm.md)（Rust→WASM）。

> 本 ADR 保留当时对「什么该上 GPU / 什么不该 / 为什么」的技术讨论记录。其性能估算和候选顺序不是现行路线；当前实时 GPU 不做，是否开展其他 GPU 工作需由具体需求另行决定。

---

## 上下文 (Context)

ADR-0007 §6 当时为「未来规模化」预留了 `ComputeBackend` trait seam（`CpuBackend` + 可选 `engine-gpu`(wgpu)，默认关）。但在本 ADR 讨论时只搭了接口骨架，没有回答一组关键问题：

1. **NPC `decide` 到底该不该搬上 GPU？** （每步都要跑、看似是最热路径）
2. **MACD/KDJ 等指标计算在哪算最划算？**
3. **撮合能不能并行？**
4. **流水线 + 三缓冲（用户提出）能不能让 GPU decide 变快？**
5. **GPU 内核确定性怎么保证？** （铁律一 TDD 要求可断言、可重放）
6. **WGSL 编译会不会是瓶颈？**

这些问题在 engine 已实现 rayon CPU 多核并行、wasm-bindgen-rayon 已跑通之后被重新审视。结论需要固化，否则后续协作者会重复踩「为什么不上 GPU」的讨论。

### 原讨论的技术基线（按当时验证记录）

- 当时记录 engine **已实现 rayon CPU 多核并行**、`wasm-bindgen-rayon` 已跑通。
- 当时对 **WGSL compute shader** 的估算为运行时一次编译 < 1ms。
- 当时估算每 tick NPC `decide` 成本约 **~5–10μs**（数十~数百 NPC），CPU↔GPU 单次 dispatch 往返约 **~200μs**。

这些估算保留为历史技术依据，不是当前性能测量、固定性能门槛或自动启动优化工作的条件。

---

## 决策 (Decision)

### §1 当前实现与保留的界面

| # | 决策 | 状态 |
|---|------|------|
| D1 | **rayon CPU 多核并行是当前最优**，作为 `CpuBackend` 基座 | ✅ 已实现 |
| D2 | MACD、价格 KDJ 与 OHLC KDJ 在 Rust engine 计算；批量输入使用 rayon 并行 | ✅ 已实现，Web 经宿主请求消费结果 |
| D3 | 连续竞价与集合竞价按股票拆分独立工作并行执行，之后按稳定股票键汇总 | ✅ 主要撮合路径已实现；不据此宣称整局所有阶段都无串行边界 |
| D4 | 蒙特卡洛 GPU 回测曾作为候选用例 | 历史候选，不是当前承诺或实施授权 |
| D5 | 保留当前 `ComputeBackend` seam，CPU 为权威后端 | ✅ 当前实现；不代表实时 GPU 路线 |

### §2 曾评估但当前不做的 GPU 方案

| # | 提案 | 判据 / 结论 |
|---|------|-------------|
| N1 | GPU compute shader for NPC `decide` | 当时的性能估算认为 dispatch 成本不合算；实时 GPU 当前不做。该估算不是现行数值门槛或未来自动触发条件。 |
| N2 | 流水线化 + 三缓冲 | 当时记录的 2-tick 延迟是该候选方案的代价；该方案未列入当前实施路线。 |
| N3 | Strategy 数据化为参数表与统一内核 | 曾作为 GPU 前置优化讨论；当前策略仍用内置代码注册，不要求为 GPU 改写。 |

表中的数量和耗时估算是当时方案评估记录，不构成当前待办、性能门槛或自动复评规则。将来如提出新的性能或 GPU 需求，按当时的具体工作负载重新评估。

### §3 当前 `ComputeBackend` 接口与边界

```rust
/// 计算后端抽象：对齐批量 NPC decide 输入与稳定顺序输出。
trait ComputeBackend: Send + Sync {
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market: &MarketView,
        selves: &[SelfView],
        seeds: &[u64],
        config: &GameConfig,
    ) -> Result<Vec<Vec<Intent>>, ComputeError>;
}

// CpuBackend 使用 rayon；Auto 当前等同 Cpu；显式 Gpu 请求返回 BackendUnavailable。
```

- 撮合、结算和路由不经 `ComputeBackend`；`engine-gpu` 只探测设备，计算仍委托 CPU。
- 保留接口是当前代码事实，不承诺通过配置即可切换真实 GPU，也不意味着实时 GPU 已进入路线。

---

## 技术细节 (Technical Notes)

以下保留当时讨论中的 GPU/计算知识记录；其中性能数值是历史估算，不代表当前机器上的复测结论：

- **T1. Compute shader ≠ 图形管线。** Compute shader 是 GPU 的「通用并行计算单元」，与顶点/片元图形渲染无关。所有 GPGPU 框架（CUDA / OpenCL / wgpu / WebGPU compute）用的是**同一个概念**——它就是「能在 GPU 上跑的数据并行内核」。
- **T2. wgpu 统一 native + Web。** wgpu 在 native 后端走 Vulkan(DX12/Metal) 各平台图形 API，在浏览器走 WebGPU 标准——**一份 WGSL 内核三端复用**（契合 ADR-0005 三端同构）。这也是我们选 wgpu 而非裸 Vulkan/CUDA 的原因。
- **T3. 整数/定点计算保证跨厂商确定性。** GPU 的浮点（f32）因厂商/驱动/融合乘加（FMA）差异**非位精确**，违反 TDD 可断言性。解决：**用整数 / 定点**（`i32`/`i64` bit 精确），与 `Money = i64 分`（定点）天然契合。浮点路径仅可用于「允许误差」的场景（如蒙特卡洛统计量），不可用于权威撮合结算。
- **T4. WGSL compute shader 编译时机：运行时一次，< 1ms。** 不是每帧重编译，也非冷启动瓶颈。流水线稳态后编译摊销为 0。
- **T5. wasm-bindgen-rayon 已跑通。** 浏览器内多核可用：nightly toolchain + `-Z build-std=std,panic_abort` + `-C target-feature=+atomics` + `wasm-bindgen-rayon`。→ CPU 多核路径在「纯前端 WASM」端也成立，不依赖 GPU。
- **T6. 流水线延迟代价 = 2-tick stale。** 若启用三缓冲流水线，decide 用的是 2 tick 前的市场快照。在**高速档位（10x+）**下一个 tick 极短，玩家对 2-tick 滞后**完全无感**；但在 1x 档位会显式感知到决策滞后。这也是流水线方案「可行但留待规模化」的原因之一。

---

## 历史方案收益分析（非当前路线）

> 下表保留原技术讨论的候选记录；已实现项标明现状，其他项目不构成实施顺序或授权。

| offload 目标 | 收益 | 难度 | 备注 |
|---|---|---|---|
| MACD/KDJ → engine Rust（rayon） | 已实现 | — | D2 当前路径 |
| 按股票拆分并行撮合 | 已实现主要路径 | — | D3 当前路径；阶段边界见 §1 |
| positions `Vec` 重构 | 未列为当前事项 | — | 历史优化候选，不要求替换现有数据结构 |
| 蒙特卡洛回测（GPU） | 未实现 | — | 历史候选，需求确定后另行评估 |
| NPC `decide` GPU 流水线 | 当前不做 | — | 历史性能估算不作为当前门槛 |

---

## 备选方案 (Alternatives Considered)

- **A. 现在就把 NPC decide 搬上 GPU** — 当时因预估往返成本较高而不选；实时 GPU 当前仍不做，该估算不作为永久触发阈值。
- **B. 用 GPU f32 做 decide + 撮合** — 否决（T3）：跨厂商非确定，违反铁律一（TDD 可断言性）。必须整数/定点。
- **C. 删除 `ComputeBackend` seam** — 当前保留 seam；其存在不代表真实 GPU 可由配置切换。
- **D. 指标计算留在前端 TS 算** — 现行 MACD/KDJ 由 engine Rust 计算，前端通过宿主消费结果。
- **E. 流水线 + 三缓冲** — 历史候选，不在当前路线。

---

## 后果 (Consequences)

- **正面：**
  - 原讨论记录了当时对 NPC GPU dispatch 的成本判断；该估算不替代未来对实际负载的测量。
  - rayon CPU 并行 + wasm-bindgen-rayon 三端一致，是当前最优且已验证的多核路径。
  - 当前 `ComputeBackend` seam 保持批量决策调用边界；未承诺真实 GPU/ML 可直接接入。
  - 整数/定点确定性方案与 `Money=i64` 契合，TDD 可断言性不破。
- **负面 / 代价：**
  - 真实 GPU 内核与 GPU 回测均未实现；是否开展需由后续具体需求决定。
- **后续需要做的：**
  - 不设本 ADR 自动触发的 GPU、三缓冲或 positions `Vec` 重构任务；未来需求应独立确定范围与验证条件。

---

## 关联 (Related)

- 细化：[ADR-0007](0007-three-deployment-frontend-framework.md) §6（GPU seam 预留、`ComputeBackend`、整数确定性）。
- 依赖：[ADR-0005](0005-unified-engine-three-deployments.md)（tick 步进、种子化 RNG、统一账户）、[ADR-0006](0006-npc-strategy-module.md)（`Strategy` trait / `Intent` / `MarketView`）、[ADR-0002](0002-engine-rust-wasm.md)（Rust→WASM、wasm-bindgen）。
- 配套：[`money` 定点设计](../superpowers/specs/2026-06-29-money-fixed-point-design.md)（Money=i64 分，整数确定性根基）。
