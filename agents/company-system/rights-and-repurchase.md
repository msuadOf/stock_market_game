# 配股／增发与回购实施台账（2026-10-07 M 批）

本目录工作文件按 [AGENTS 约定](../../CLAUDE.md) 归档。本批实现 ADR-0038/0039
（含 2026-10-07 决策注记）授权的配股／增发与回购机制，及两个独立新局开关。

## 研究结论（两地口径与证据缺口）

取证基础为已归档的官方全文（`agents/company-system/remaining-corporate-rules-research.md`
与 `.tmp/company-system/corporate-rules-followup/`），本批另经 WebSearch 核验现行有效性：

- **配股（深市有据）**：深业〔2025〕68号 2.4.3（一）—（五）/2.4.4（一）（二）
  ——R-3 前申请、R 日日终派发配股权证、缴款期起始日至 L 日报盘认购、L+2 扣登记费
  后划款（含利息）至主承销商备付金账户、失败 L+2 退认购资金及利息、碎权证按数量
  降序＋同数系统随机各登记 1 份。206号令第 53 条：配股 ≤ 配售前股本 50%、代销、
  认购 < 70% 拟配售量即发行失败。缴款期天数指南未固定 → 参数化。
- **配股（沪市证据缺口，登记）**：沪业字〔2024〕6号第 2.4 节无权证/缴款期/L+2
  划款退款/碎权证操作条文，仅确认注册、募集资金到位说明、回购专户不享有配售权
  （后者为沪市明文、两市共用上位 63 号第 13 条）。按「深市有据口径实现＋沪市登记
  简化/待证」处理，不拿深市冒充沪市。
- **配股除权日 = L+1：实践口径待证**（实际认购比例 L 日才确定；交易所除权条文
  只给公式未给配股除权日）。
- **回购现行依据**：证监会《上市公司股份回购规则》〔2023〕63号已经〔2025〕5号
  修改（新《公司法》衔接）——本批据此修正任务书引用，以现行有效版本为依据；
  沪指引 7 号（2025 年 3 月修订，现行有效）第 22 条与 63 号第 13 条同文确认专户
  失权。集中竞价委托限制按 63 号第 30 条已核全文实现（不以涨停价申报、不集合竞价
  时段申报）；窗口禁止（第 31 条）与每日数量限制未取得完整现行可实施原文 →
  显式登记不支持。
- **注销不除权**：交易所除权公式只覆盖股份增加情形，注销无除权条文、市场实践
  不除权；无官方明文 → 登记为待证口径。
- **合成资金差额**：无官方规则（ADR-0038 游戏授权域）→ 口径：计划完成时未用
  差额显式回收、不落地为任何人余额。

## 红绿日志索引（`.tmp/company-system/rights-repurchase/`）

| 阶段 | 红 | 绿 |
| --- | --- | --- |
| 新局开关（严格持久化） | `switch-red.log`（JSON 层断言失败） | `switch-green.log`、`web-switch-green.log` |
| 配股 company 模块 | `rights-module-compile-red`（会话接线前编译红在 `session-wiring-red.log`） | `rights-module-green.log`（7/7） |
| 回购 company 模块 | 同上（新模块编译红） | `repurchase-module-green.log`（8/8） |
| 配股 Session 全链路 | `session-wiring-red.log`（缺 API/字段编译红） | `rights-session-green.log`（7/7） |
| 回购 Session 全链路 | 同上 | `repurchase-session.log`（4/4） |
| Web 契约 | `web-full.log`（finance 字段缺失红） | `web-final.log` |
| 基线对照 | `head-failures.txt`（main@78a35747 lib 257 失败清单） | `engine-lib-full.log`（同 257 失败、净增 33 通过）；Web 既有失败集与 main 一致 |

## 验证范围

只做定向短单测、独立编译与生产构建，未运行完整回归（按批次纪律）。各组命令外部
deadline 10000ms、编译/构建/长验收 300000ms，多核并行（rustc 三 producer 并行、
Web 测试 shard 并行）。证据日志见上表目录。

| 验证 | 结果 |
| --- | --- |
| `company::rights_offering` 模块 | 7/7 |
| `company::issuer_repurchase` 模块 | 8/8 |
| `company::ex_reference_price`（含配股公式） | 19/19 |
| `session::rights_offering_session_tests` | 7/7 |
| `session::issuer_repurchase_session_tests` | 4/4 |
| 受影响组并发复跑（rights/repurchase/share_registry/ex_reference_price/corporate_actions/cash_dividend/company_mechanism/dividend_tax/company_simple 过滤） | 162/162 |
| engine lib 全量 | main@78a35747 基线 257 失败逐项一致；本批 1472 通过（净增 33） |
| `cargo check --workspace --all-targets` | 通过（含 desktop；web/dist 由本批构建产出） |
| Web `scripts/run-web-tests.mjs` | main 基线既有失败集一致，本批零新增失败 |
| Web 生产构建（tsc -b + vite + release WASM 校验） | 通过 |
| ts-rs typegen + `check-generated-types` | 通过 |
| 三份存档 fixtures（release Engine producer 重生成） | restore/resave 深等与场景守卫全过 |

## 独立复核门禁（大 A 语义）

本 diff 须由未实施本批的 subagent 按 CLAUDE.md 复核：语义依据、最小范围、
边界测试与跨层语义漂移。本文件留待复核结论回填（复核记录建议另存
`rights-and-repurchase-review.md`）。
