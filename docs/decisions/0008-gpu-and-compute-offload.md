# ADR-0008: GPU 与计算 offload 技术分析（结论与决策）

- **状态 (Status):** accepted
- **日期 (Date):** 2026-06-30
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** 细化 [ADR-0007](0007-three-deployment-frontend-framework.md) §6（GPU seam 预留）；依赖 [ADR-0005](0005-unified-engine-three-deployments.md)（tick 步进 / 种子化 RNG）、[ADR-0006](0006-npc-strategy-module.md)（Strategy trait）、[ADR-0002](0002-engine-rust-wasm.md)（Rust→WASM）。

> 本 ADR 是一次**深度技术讨论**的沉淀：把「什么该上 GPU / 什么不该 / 为什么」的推理固化下来，避免日后反复重提同一组问题。**结论先行：当前规模 GPU 是负收益，但为真正的用例（蒙特卡洛回测、超大规模 NPC）留好 seam。**

---

## 上下文 (Context)

ADR-0007 §6 为「未来规模化」预留了 `ComputeBackend` trait seam（`CpuBackend` + 可选 `engine-gpu`(wgpu)，默认关）。但当时只搭了接口骨架，没有回答一组关键问题：

1. **NPC `decide` 到底该不该搬上 GPU？** （每步都要跑、看似是最热路径）
2. **MACD/KDJ 等指标计算在哪算最划算？**
3. **撮合能不能并行？**
4. **流水线 + 三缓冲（用户提出）能不能让 GPU decide 变快？**
5. **GPU 内核确定性怎么保证？** （铁律一 TDD 要求可断言、可重放）
6. **WGSL 编译会不会是瓶颈？**

这些问题在 engine 已实现 rayon CPU 多核并行、wasm-bindgen-rayon 已跑通之后被重新审视。结论需要固化，否则后续协作者会重复踩「为什么不上 GPU」的讨论。

### 关键事实基线（讨论时已验证）

- engine **已实现 rayon CPU 多核并行**，是当前最优的多核方案。
- **wasm-bindgen-rayon 已跑通**（nightly toolchain + `build-std` + `-C target-feature=+atomics`）→ 浏览器内也能拿多核，CPU 路径三端一致。
- **WGSL compute shader 运行时一次编译 < 1ms**（非瓶颈，见 §技术细节 T4）。
- 当前每 tick 的 NPC `decide` 总成本约 **~5–10μs**（数十~数百 NPC 量级）。
- CPU↔GPU 单次 dispatch 往返约 **~200μs**（数据上传 + dispatch + 回读 + 同步）。

---

## 决策 (Decision)

### §1 当前采纳（要做 / 已在做）

| # | 决策 | 状态 |
|---|------|------|
| D1 | **rayon CPU 多核并行是当前最优**，作为 `CpuBackend` 基座 | ✅ 已实现 |
| D2 | **MACD / KDJ 指标计算搬到 engine Rust**（rayon 加速，前端减负）—— 指标本是纯函数、批量算，rayon 直接受益 | 待落地 |
| D3 | **跨股票并行撮合**（`par_iter` 按股票切片）—— 各股票 orderbook 独立，天然可并行 | 待落地 |
| D4 | **蒙特卡洛回测是 GPU 的真正用例**（离线、独立、大数据量、对延迟不敏感）—— 列为未来 GPU 首个落地目标 | 已标记为 GPU 用例 |
| D5 | **保留 `ComputeBackend` trait seam**，为未来 GPU/ML 铺路（不删除 ADR-0007 §6 的预留） | ✅ seam 在 |

### §2 已分析但当前不做（且有明确判据，避免日后再被反复提）

| # | 提案 | 判据 / 结论 |
|---|------|-------------|
| N1 | **GPU compute shader for NPC `decide`** | **当前负收益。** CPU↔GPU 往返 ~200μs，而当前每步 CPU 仅 ~5–10μs → **GPU 慢约 40 倍**。只有单步 decide 计算量显著大于 200μs 时才有意义，那需要每步远超当前规模。**不做。** |
| N2 | **流水线化 + 三缓冲（用户提出）** | **架构上可行，是「延迟换吞吐量」的经典 trade-off。** 流水线会把 decide 滞后 2-tick（stale 数据）。但该方案**只有在 NPC 数量 5000+ 时吞吐量收益才显现**——当前规模无收益反而引入复杂度。**当前不做，留作规模化方案。** |
| N3 | **Strategy 数据化（`Box<dyn Strategy>` → 参数表 + 统一内核）** | 这是 **GPU 化的前提**（GPU 要数据并行、SOA 布局、不能跑动态分发）。但其本身是独立重构、影响面大、当前 CPU 下 trait 对象开销可忽略。**单独做，不绑死在 GPU 议题上。** |

> 三条「不做」的判据都写死在上表。**日后若有人再提，请先回答：NPC 规模是否已达 5000+？单步 decide 是否已 > 200μs？** 若否，维持本 ADR 结论。

### §3 `ComputeBackend` trait 设计草案（seam，保留）

```rust
/// 计算后端抽象：让 NPC decide / 指标 / 演化的执行位置可切换（CPU ↔ GPU）。
/// 不强制现在实现 GpuBackend；trait 在 = 给未来留接口、给配置开关留位置。
trait ComputeBackend: Send + Sync {
    /// 提交一批 decide（异步语义）；CpuBackend 同步执行，GpuBackend 异步 dispatch。
    fn submit_decide(&self, snapshot: &TickSnapshot, params: &[NpcParams], seeds: &[u64]) -> PendingDecide;
    /// 轮询结果；CpuBackend 立即可得，GpuBackend 需等 GPU 完成。
    fn poll_decide(&self, pending: PendingDecide) -> Vec<Vec<Intent>>;
    /// 演化（指标/状态推进），就地更新 markets。
    fn evolve_v(&self, markets: &mut [Market], params: &VParams, seeds: &[u64]);
}

// CpuBackend : ComputeBackend  → rayon 同步执行（当前唯一实现）
// GpuBackend : ComputeBackend  → wgpu 异步 dispatch + 轮询（未来）
// ComputeMode { Cpu, Gpu, Auto } → 配置切换；默认 Cpu。
```

- `CpuBackend` 是当前唯一实现（rayon 同步），trait 的存在只为了让「换 GPU」成为**配置切换而非改代码**。
- `Auto` 模式留位：未来可按规模自动选 CPU/GPU，但当前 `Auto == Cpu`。

---

## 技术细节 (Technical Notes)

讨论中澄清的、容易被误解的几条 GPU/计算知识，固化以防重复踩坑：

- **T1. Compute shader ≠ 图形管线。** Compute shader 是 GPU 的「通用并行计算单元」，与顶点/片元图形渲染无关。所有 GPGPU 框架（CUDA / OpenCL / wgpu / WebGPU compute）用的是**同一个概念**——它就是「能在 GPU 上跑的数据并行内核」。
- **T2. wgpu 统一 native + Web。** wgpu 在 native 后端走 Vulkan(DX12/Metal) 各平台图形 API，在浏览器走 WebGPU 标准——**一份 WGSL 内核三端复用**（契合 ADR-0005 三端同构）。这也是我们选 wgpu 而非裸 Vulkan/CUDA 的原因。
- **T3. 整数/定点计算保证跨厂商确定性。** GPU 的浮点（f32）因厂商/驱动/融合乘加（FMA）差异**非位精确**，违反 TDD 可断言性。解决：**用整数 / 定点**（`i32`/`i64` bit 精确），与 `Money = i64 分`（定点）天然契合。浮点路径仅可用于「允许误差」的场景（如蒙特卡洛统计量），不可用于权威撮合结算。
- **T4. WGSL compute shader 编译时机：运行时一次，< 1ms。** 不是每帧重编译，也非冷启动瓶颈。流水线稳态后编译摊销为 0。
- **T5. wasm-bindgen-rayon 已跑通。** 浏览器内多核可用：nightly toolchain + `-Z build-std=std,panic_abort` + `-C target-feature=+atomics` + `wasm-bindgen-rayon`。→ CPU 多核路径在「纯前端 WASM」端也成立，不依赖 GPU。
- **T6. 流水线延迟代价 = 2-tick stale。** 若启用三缓冲流水线，decide 用的是 2 tick 前的市场快照。在**高速档位（10x+）**下一个 tick 极短，玩家对 2-tick 滞后**完全无感**；但在 1x 档位会显式感知到决策滞后。这也是流水线方案「可行但留待规模化」的原因之一。

---

## 收益排序表 (Payoff Ranking)

> 决定**做事顺序**的依据：先做低难度高收益的，GPU 高难度低（当前）收益的放最后。

| offload 目标 | 收益 | 难度 | 备注 |
|---|---|---|---|
| MACD/KDJ → engine Rust（rayon） | ⭐⭐⭐ | 低 | D2，优先做；指标纯函数 + 批量，rayon 直接受益，前端减负 |
| 跨股票并行撮合（`par_iter`） | ⭐⭐ | 低 | D3，orderbook 跨股票独立，天然并行 |
| positions `Vec` 重构（消除 `deepNormalize`） | ⭐⭐ | 低 | 顺带消除每 tick 的深度规范化开销 |
| 蒙特卡洛回测（GPU） | ⭐⭐⭐ | 中 | D4，**GPU 真正用例**：离线、独立、大数据量、对延迟不敏感 |
| NPC `decide` GPU 流水线 | 当前**负收益** | 高 | N1/N2，需 5000+ NPC 才回正；现在做反而慢 40 倍 |

---

## 备选方案 (Alternatives Considered)

- **A. 现在就把 NPC decide 搬上 GPU** — 否决（N1）：往返 200μs vs 当前 5–10μs，慢 40 倍。负收益。判据写死在 §2。
- **B. 用 GPU f32 做 decide + 撮合** — 否决（T3）：跨厂商非确定，违反铁律一（TDD 可断言性）。必须整数/定点。
- **C. 删掉 `ComputeBackend` seam，等真要 GPU 再加** — 否决：seam 成本极低（一个 trait + `ComputeMode` 枚举），且让「换后端」=配置切换而非改代码；删了反而把未来路堵窄。保留（D5）。
- **D. 指标计算留在前端 TS 算** — 否决（D2）：前端算重复了 engine 已有的价格序列、加重 UI 线程、跨端可能算出不一致结果。搬到 engine Rust 统一算、rayon 加速、前端只展示。
- **E. 流水线 + 三缓冲现在就上** — 否决（N2）：架构可行，但当前规模无吞吐收益、且引入 2-tick stale 复杂度。留作 5000+ NPC 时的方案。

---

## 后果 (Consequences)

- **正面：**
  - 「该不该上 GPU」有了**有判据的结论**，不再反复讨论；判据可量化（NPC ≥ 5000？单步 > 200μs？）。
  - rayon CPU 并行 + wasm-bindgen-rayon 三端一致，是当前最优且已验证的多核路径。
  - `ComputeBackend` seam 保留 → 未来 GPU/ML 落地是「加实现 + 配置切换」，不改调用方。
  - 蒙特卡洛回测被明确为 GPU 首个落地目标，方向聚焦。
  - 整数/定点确定性方案与 `Money=i64` 契合，TDD 可断言性不破。
- **负面 / 代价：**
  - D2/D3（指标搬 engine、跨股票并行撮合）是**待落地的实打实工作量**，本 ADR 只定方向。
  - 蒙特卡洛 GPU 回测（D4）依赖 wgpu 重依赖 + 整数内核，是中难度项，需单独设计。
  - Strategy 数据化（N3）作为 GPU 前提被单独列项，未来若启动 GPU decide 必须先做它。
- **后续需要做的：**
  - **优先**：D2（MACD/KDJ 搬 engine）→ D3（跨股票并行撮合）→ positions Vec 重构。三者低难度高收益。
  - **后续**：蒙特卡洛回测 GPU 化设计（独立 spec/ADR，落实整数内核 + wgpu dispatch）。
  - **触发条件**：当 NPC 规模逼近 5000 或单步 decide > 100μs 时，重新评估 N1/N2（流水线 + Strategy 数据化 N3）。
  - 本 ADR 不新增开放问题（讨论已闭环）；如规模演进触发重评，届时新建 ADR 推翻/细化本条。

---

## 关联 (Related)

- 细化：[ADR-0007](0007-three-deployment-frontend-framework.md) §6（GPU seam 预留、`ComputeBackend`、整数确定性）。
- 依赖：[ADR-0005](0005-unified-engine-three-deployments.md)（tick 步进、种子化 RNG、统一账户）、[ADR-0006](0006-npc-strategy-module.md)（`Strategy` trait / `Intent` / `MarketView`）、[ADR-0002](0002-engine-rust-wasm.md)（Rust→WASM、wasm-bindgen）。
- 配套：[`money` 定点设计](../superpowers/specs/2026-06-29-money-fixed-point-design.md)（Money=i64 分，整数确定性根基）。
