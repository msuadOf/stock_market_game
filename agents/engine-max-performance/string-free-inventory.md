# Engine 无字符串化清点清单（ADR 0041 升级版）

用户决策（2026-10-06）：引擎核心运行时**零 String**。标识一律数值 id，类别一律 enum，
错误一律结构化（错误码 enum + 数值/枚举上下文），显示文本只在边界格式化。
本清单是完成标准：**下列各项全部归零（或落入"边界例外"并说明）才算完成**。
状态更新随提交同步。

## 清点口径

- 范围：`packages/engine/src` 非测试代码。测试代码可自由用字符串构造期望值。
- serde/ts_rs 序列化产物（JSON 字段名、十进制金额字符串）不算运行时 String，
  但承载它们的**运行时字段类型**必须数值/枚举。
- `format!` 仅允许出现在：边界格式化函数、测试、诊断输出（cfg(feature) 门控）。

## A. 标识符数值化（必须归零）

| 项 | 现状 | 目标 | 状态 |
|---|---|---|---|
| 股票 StockCode(pub String) | 全引擎 ~1222 处引用 | `StockId(u32)` + `StockCatalog` 目录翻译（id 序=代码序） | **进行中**（已全量改名，编译修正中） |
| 公司 CompanyId(pub String) | company/* 全子系统以字符串 id 关联 | `CompanyId(u32)` 数值化，公司名进目录/元数据 | **已完成**（42ac1d9b 清尾：CompanyCatalog 公开重导出、测试夹具全量对齐） |
| 证据投影 EnvelopeKeyProjection { account_id: String, stock_code: String, order_id: String, side: &'static str } | verification_evidence | 全部数值/枚举字段 | 未开始 |
| 拍卖/连续订单归属 venue: String / frozen: String | player_orders（值域 "auction"/"cash"/"shares"） | enum OrderVenue / FrozenAsset | 未开始 |
| E3 排序元组局部 u8 股票序数 | continuous_matching_adapter | 统一用全局 StockId | 随 A 项归零 |
| calendar/information 光标 cursor: String、company: String | information/mod.rs | 光标数值化/结构化 | 未开始 |

## B. 错误结构化（必须归零）

- 全引擎 ~66 处 `reason/detail/text/label: String` 错误载荷 + ~215 处 `&'static str`
  字段标签（"cash" 之类）。多数集中在：
  accounting/error.rs、company/error.rs、company/operations/error.rs、information/mod.rs、
  calendar.rs（field: &'static str）、experience.rs、observation.rs、config.rs、
  verification_evidence.rs、session/failure.rs、compute.rs。
- 目标形态：每个错误 enum 的载荷只有数值/枚举/既有强类型（Money、AccountId…）；
  `InvalidSetup(String)` 等自由文本 → 具名变体 + 枚举上下文。
  Display 实现改为边界调试用（可保留非契约）；**首错序测试从文本断言改为结构断言**
  （语义等价的正确形态，ADR 登记）。
- 状态：未开始（依赖 A 项完成，避免双大 diff 交织）。

## C. 类别枚举化（必须归零）

- player_orders 的 venue/frozen（见 A 表）。
- 其余散点 `String` 类别字段以编译器+grep 逐个清点归零（清点时补表）。

## D. 序列化边界投影（允许保留，登记口径）

- company/query.rs 约 61 处 String：多为**投影结构**（存档/查询 API 的 JSON 输出），
  其中金额类 String 是 `u64_decimal` JSON 安全约定（防超 JS 安全整数）——保留序列化
  形态，但其来源运行时字段必须已是 Money/u64；公司 id 字符串随 CompanyId 数值化改 u32。
- verification_evidence 投影中面向审计 JSON 的 display 字段：随 A/B 项数值化后，
  边界格式化统一走目录翻译。

## E. 边界例外（单列，说明原因）

- **领域内容文本**：公司名称、披露公告正文、财报附注叙述等——这些是**游戏内容数据**
  （生成/配置产生，需逐字节保留与回放），不是标识符；保留 String，存于内容结构，
  键位一律数值。例外理由：数值化内容无意义且破坏内容完整性。
- **配置加载**：StockSpec.code 等输入侧文本（边界→id 翻译入口）。
- **诊断输出**：cfg(feature = "simulation-diagnostics") 门控的调试文本。

## 显式遗留（不静默）

- **IndustryId(pub String)**（行业标签，fixtures 用 "home-appliances" 等）：跨 company/information/shock 子系统，量大且独立成批，未随 CompanyId 批数值化；登记为下一批候选。
- **CounterpartyId(pub String)**（对手方命名空间）：外部对手方本就是字符串名（"EXT-CUST" 等），与"内部公司对手方经目录翻译"的类型隔离并存；是否数值化待设计定夺。
- **LedgerAccountId / InventoryItemCode / FixedAssetCode 等会计内容码**：登记为内容码（生成/配置产生、需逐字节保留），非标识符，保留 String（见上文例外）。

## 执行顺序

1. A-股票（进行中）→ 提交 A（引擎核心 StockId + 目录 + 存档/事件破坏性变更）
2. B-错误结构化 → 提交 B（错误 enum 重构 + 首错序测试改结构断言）
3. A-公司 CompanyId + 其余标识（venue/frozen/光标）→ 提交 C
4. server wire + ts_rs 再生成 + web 最小编译修复 + 配置格式 → 提交 D
5. 基准（锚定交错配对）+ findings + ADR 终稿
