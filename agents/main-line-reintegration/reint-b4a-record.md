# B4a 批落地记录（公司纯域闭包回归）

> 实现 agent 产出的批次台账。分支 `feat/reintegration-b4a-company-domain`
> （worktree `.worktree/reint-b4a`），基 main `fe9e2ff5`。
> 素材锚点：main 侧终态 `18725d07`，分叉点 `a6001494`。

## 提交序列（6 刀）

| 提交 | 内容 |
|---|---|
| `ce66e94f` | accounting 账务域（ROE 三件套+分录变体+报表扩展+num-bigint+calendar） |
| `9f3ebd4c` | information 披露域扩展（月报排期/AnnouncementContent 公司公告/ReportFrequency） |
| `3a20c524` | 公司域主体（公司行为状态机/税务/名册/Simple 财务/identity/system+行业对齐） |
| `884bd5ad` | 收口刀（AccountId canonical serde/market price_bound 单一化/session·tests 适配） |
| `9b395dd5` | ts_rs 生成物同步（62 新类型+AccountId string 化） |
| `8c406fdd` | Cargo.lock 补 |

## 落地范围（对照盘点 §4）

- **账务/分录**：accounting 全对齐（disclosure_roe/period_roe/reports/roe/simple_summary + journal 分录变体 + closing/consolidation/reports 演进）
- **税务**：cash_dividend_tax（差别化持有期）+ 四行业 income_tax + claim_sources + owner_state
- **公司操作域**：cash_dividend/stock_distribution/share_split/rights_offering/issuer_repurchase/share_registry/ex_reference_price/dividend/capabilities/identity/correction/report_correction/persistence/config/api/system
- **Simple 周期/财务**：simple/ 30 文件（周期生成/年化/逐项分录/勾稽/偏好）
- **域测试**：对 18725d07 基线重立并数值化适配（拆股域含 7+ 例测试随域落地）
- **不含**（按任务书）：corporate_actions.rs 日终落账（B4c）、AdminRight 接线（B4b/d）、
  operations/restore.rs（save/restore 校验链，B4b）、orderbook 受理域扩展（B4c/d）、
  host ~40 pub 方法面（B4e）、web 组件（W3/B4f）

## std-thread 架构适配点（String→数值 CompanyId）

1. `identity::IssuerRegistry`：按 spec.code 升序分配数值 id（CompanyCatalog 同款约定）；
   `code_of/id_of_code` 提供边界翻译
2. `schedule::ReportFrequency::*` 与 `monthly_schedule::delay_days` 的公司参数改
   `&str` 代码（排期偏移 hash 输入保持 main 侧语义），调用方（prehistory/
   simple_disclosures）经目录翻译
3. `spec.rs` serde 面适配 HEAD 形态（code 字段 + canonical issued_shares +
   required_nullable），main 侧 spec 测试重立
4. `report_correction`/ROE/域测试的 `CompanyId("…")`/`MemberId("…")` 按文件内
   代码序数值化；**教训**：同文件多身份需保证映射不撞号（period_roe 曾因
   分两轮映射 issuer→0/other→0 撞号，scope mismatch 消失）
5. `simple/state` 周期生成 par_iter→串行（引擎不引 rayon，std-thread 决策）
6. `company::error::CompanyErrorClass` 四分类 + `IndustrialError` 补 main 变体
   （保留 HEAD 的 HistoricalTaxReassessmentUnsupported——HEAD 测试仍引用）

## 公共文件决策

- **AccountId serde 切 canonical_u64_decimal**（与 main 对齐，`ts(type="string")`）：
  域存档断言（share_registry `{"Account":"2"}`）依赖字符串形态；局部 with 会造成
  同类型双存档形态的跨层漂移，故全局切换。OrderId/seq 仍 js_safe（main 同款）。
  `ledger_paged_map` 的 JS 安全整数断言随语义退役改写（canonical 全域无损）
- **money.rs 零改动**（main 侧无 diff；盘点的 94 行是 HEAD↔main 双向差）
- **market::price_bound_cents**：共享纯函数 + `Market::price_bound` 重接（单一事实源）

## 验证结论

- lib：**2194 passed / 0 failed**（基线 1773；+421 域测试全绿）
- tests 集成面：**2416 passed / 0 failed**（**基线 15 个既有失败全部转绿**：
  save_contract×5、step_skeleton 特征锚×3、player_snapshot、civil_replay、
  full_day 等——main 行业恢复校验链顺带修复；无新增失败）
- serial 逃生门（ENGINE_STEP_RUNTIME=0）：memberships 1 个失败与基线**完全一致**
  （既有）；时序敏感测试（watchdog/skeleton 时序/desk r09）在满载机器上基线与
  批后均偶发（基线 4 连跑 1 次失败），单跑全过，非本批引入
- wasm32 target 编译通过；web tsc 0 错误
- step_skeleton 三特征锚重钉（旧锚注释保留，依据=AccountId 存档字节+公司结构）

## 架构符合性声明

域模块零 `crate::session`/`crate::runtime` 代码引用（rng.rs 仅注释提及、
insurance/session_restore_tests.rs 为基线既有测试）；`runtime/` 目录零改动
（AdminRight/rights_computer/组线程未触碰）；涨跌停推导单一事实源
（price_bound_cents 共享核心）；公司域公式与 B2 观察面/费用面无重叠（分红税/
除权价均为新域纯函数，DeskTradeFact 费用面未重复实现）。

## 遗留与交接

1. **worktree 交叉**：03:02 起 W3 实现 agent 在同一 worktree 产出（12 个组件
   文件+host/save 接线 M）——本批提交全部显式 pathspec 排除之，合并前需
   W3 agent 收口或分 worktree
2. **save_contract 基线 5 失败的转绿根因**：main 行业 Books Deserialize 校验链
   修复了 HEAD 侧存档篡改检测分层（错误从 restore 层提前到 decode 层且带语义
   文本）——复核时值得独立确认该行为变化符合预期
3. B4b 输入已就位：identity/system/装配域（IssuerRegistry/CompanySystem/
   CompanySpec serde 面）与 SessionSetup/save 面的接缝即盘 #11/#12
