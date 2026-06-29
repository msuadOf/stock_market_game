# ADR-0006: NPC 策略模块（Strategy trait + 每实例独立参数 + 可插拔扩展）

- **状态 (Status):** accepted
- **日期 (Date):** 2026-06-29
- **决策者 (Deciders):** msuad + Claude
- **解决：** [open-questions.md](../open-questions.md) Q11（NPC AI 行为模型）。
- **关联：** [ADR-0005](0005-unified-engine-three-deployments.md)（统一账户模型、撮合驱动价格）。

## 上下文 (Context)

ADR-0005 定调「统一账户 + 共享盘口 + 撮合驱动价格」后，NPC 是**主动挂单的 AI 参与者**，与玩家平权进同一 orderbook。但三类 NPC（散户 retail / 机构 inst / 游资 hot）各自的**下单策略算法**此前未定（open-questions Q11，阻塞 market/account 的 NPC 部分）。

研究（市场微观结构 + agent-based 模拟文献，见本 ADR §研究基础）结论：所有交易者都可抽象为「按策略决定下单意图」的函数，区别仅在策略类型与参数。两类奠基模型可对接到我们的三类 NPC：
- **零智力（Zero-Intelligence, ZI）模型**（Gode & Sunder / Cont-Stoikov-Talreja）：限价单、市价单、撤单按泊松到达率随机产生，不看信息。→ 散户噪音交易本体。
- **基本面 vs 趋势**（MDPI 2023）：期望收益 = 股息率 + 基本面项(价值-价格缺口) + 趋势项(动量偏好)。→ 机构(价值) / 游资(动量)本体。

msuad 2026-06-29 对策略模块给出明确要求：
1. **策略必须是独立模块**，与 account 解耦。
2. **每个 NPC 实例参数独立**（不同散户算法可不同、不同机构参数可不同）——同类内部也要有差异，市场才生动。
3. **后续要能插拔式添加更多策略**，不改现有代码（开闭原则）。
4. 采纳研究的三类映射：散户=ZI噪音、机构=价值、游资=动量。

此外 msuad 对「机构公允价」给出关键细化：**市场存在一个隐藏的「真实价值 V」轨道**（基本面随机游走，玩家不可见）；但**不同机构对 V 的「目标价/看法」可以不同**（可配置：固定目标价 / 跟随 V 带看法偏差 / 认为大致上涨）——即 V 是事实，机构的**预期**是各自参数。

## 决策 (Decision)

### §1 策略是 engine 内的独立模块

- 路径：`packages/engine/src/strategy/`（模块内可按策略拆文件：`mod.rs` + `trait.rs` + `zi_noise.rs` + `value.rs` + `momentum.rs`）。
- 不单独成 crate：策略是纯逻辑、与 orderbook/market/account 强耦合，TDD 同栈最顺；若日后逻辑膨胀再拆 crate（YAGNI）。

### §2 核心 trait：纯函数式决策

```rust
/// NPC 下单策略的统一抽象。看市场快照 + 自己的 RNG，返回一个「意图」。
/// 策略不直接碰 orderbook——只产 Intent，由 account/market 层执行。
/// 这让策略可单测、可插拔、可并行。
pub trait Strategy {
    fn decide(&mut self, ctx: &MarketView, rng: &mut Rng) -> Intent;
}
```

- `MarketView`：只读市场快照（当前盘口买卖档、近期价格序列、成交量、**隐藏公允价 V 仅对有权看它的策略可见**）。
- `Intent`：决策产物——枚举 `{ PlaceLimit{side, price, qty}, PlaceMarket{side, qty}, Cancel{order_id}, Pass }`。
- **RNG 由调用方注入**（呼应 ADR-0005 种子化 PRNG）→ 策略决策可重放、可断言（TDD）。

### §3 每实例独立参数（同类 NPC 内部也有差异）

- 每个策略 struct **自带参数**（如 `ZiNoiseStrategy { arrival_rate, order_size_mean, ... }`）。
- `GameConfig` 提供**分布参数**（均值/方差/范围），引擎为**每个 NPC 实例**采样出具体值 → 同类不同参数。例：两个散户的 `arrival_rate` 各自从 N(μ, σ²) 采样。
- 落实「不同散户算法不一样、不同机构参数不一样」的要求。

### §4 可插拔扩展（开闭原则）

- 加新策略 = 新增一个实现 `Strategy` 的 struct + 注册到「策略工厂」（`enum StrategyKind` + `fn build(kind, params, rng) -> Box<dyn Strategy>`）。
- **不改任何现有策略代码**。`Account` 持有 `Box<dyn Strategy>`（玩家为 `None`），对新策略透明。

### §5 隐藏公允价 V 轨道 + 机构各异的目标价

- 每只股票的 `MarketState` 持有一个**隐藏** `fundamental_value: Money`（基本面随机游走，玩家不可见；其随机性纳入 Session 的种子化 RNG）。
- 机构的「目标价」是**各实例参数**，三种可配策略（msuad 定）：
  - `Fixed(Money)`：固定目标价（机构对该公司有固定估值）。
  - `TrackV { bias: f64 }`：跟随 V，但带各自的看法偏差（bias）。
  - `DriftUp { rate: f64 }`：认为大致上涨（线性/指数漂移）。
- 同一个 V，不同机构看法不同 → 符合「不同机构看法不一样」。

### §6 玩家账户（不走 Strategy）

- `Account` 持 `strategy: Option<Box<dyn Strategy>>`。玩家为 `None`。
- 玩家下单：UI 动作直接构造 `Intent`（经请求通道注入），不经过策略 trait。
- 体现了「NPC=算法、玩家=外部驱动」的统一账户模型。

### §7 首批三策略（采纳研究映射）

| 策略 | 服务 NPC | 本体 | 关键参数（每实例采样） |
|------|---------|------|------------------------|
| `ZiNoiseStrategy` | 散货 retail | ZI 泊松 + 少量追涨杀跌 | arrival_rate λ、order_size_mean、trend_weight(小)、cancel_rate、holding_horizon(短) |
| `ValueStrategy` | 机构 inst | 基本面价值 + 拆单 | target_price_policy(Fixed/TrackV/DriftUp)、value_weight(高)、order_size_split(拆单)、holding_horizon(长)、fair_value_deviation(小) |
| `MomentumStrategy` | 游资 hot | 强趋势/动量 + 短线 | trend_weight(高)、lookback(趋势窗口)、order_size_mean、holding_horizon(极短)、cancel_rate(高) |

## 研究基础 (Research Basis)

- **Winner Strategies in a Simulated Stock Market**（MDPI, Int. J. Financial Studies 2023）— 基本面/趋势双因子、参数 (cat, dur, tfp)、相对强度决定胜负。
- **Simulating Limit Order Book Models**（Watts）+ **Cont, Stoikov & Talreja (2010)** — 零智力泊松到达模型（限价/市价/撤单）。
- **Noise Traders**（NBER w12256）— 噪声交易者作为流动性与偏差来源。
- 详见上一轮研究记录（已纳入对话）。

## 备选方案 (Alternatives Considered)

- **A. 策略写成单独 crate** — 解耦更彻底，但当前与 engine 耦合紧密、TDD 跨 crate 更绕；YAGNI，待膨胀再拆。不选。
- **B. 三类 NPC 共用一套策略、仅参数不同** — 违背「不同散户算法可不同」（要能加新算法，非仅改参数）。不选。
- **C. 玩家也套 Strategy 外壳（HumanStrategy）** — 为统一性硬套一层无意义抽象；玩家本质是外部驱动。不选。
- **D. 无隐藏 V，机构退化为长线均值回复者** — 机构「价值判断」变弱、失去公允价依据；msuad 明确要隐藏 V。不选。

## 后果 (Consequences)

- **正面：**
  - 策略与账户解耦，account/market/orderbook 不依赖具体策略 → 可独立 TDD。
  - 每实例独立参数 + 可插拔 trait → 后续加策略零侵入，市场生动。
  - 隐藏 V + 机构各异目标价 → 机构行为有真实依据且多元化。
  - RNG 注入 → NPC 决策可重放、可精确单测（TDD）。
- **负面 / 代价：**
  - 多一个隐藏 `fundamental_value` 轨道（市场状态体积与复杂度↑），其随机游走参数需配。
  - trait 对象（`Box<dyn Strategy>`）有微小动态分发开销——对 NPC 级数量级（数十~数百）可忽略。
  - 三策略 + 分布参数 + 工厂是首批实打实的工作量。
- **后续需要做的：**
  - 解决 [open-questions.md](../open-questions.md) Q11（标已解决）。
  - 实现顺序（在 account/orderbook 之后或并行）：先 `Strategy` trait + `Intent` + `MarketView`，再三策略，再工厂。
  - `GameConfig` 扩展：每类 NPC 的分布参数 + 隐藏 V 的随机游走参数。
  - 后续每加一个新策略，补一个实现 + 单测 + 工厂注册项。

## 关联 (Related)

- 解决：[open-questions.md](../open-questions.md) Q11。
- 依赖：[ADR-0005](0005-unified-engine-three-deployments.md)（统一账户、撮合驱动价格、种子化 RNG）。
- 配套：[`money` 设计](../superpowers/specs/2026-06-29-money-fixed-point-design.md)、[`GameConfig` 设计](../superpowers/specs/2026-06-29-gameconfig-design.md)、后续 `account`/`orderbook`/`market` 设计。
