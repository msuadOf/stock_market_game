# CompanySystem 当前交接

## 已接通的代码

`packages/engine/src/company/` 是共同入口，`simple/` 已独立承接基本面生成与汇总财务；`GameSession` 不再运行旧 `CompanyOperations` 后台。`Simulation` 当前明确拒绝创建，后续由用户另开分支实现，不做默认降级、格式兼容或 schema 迁移。

- 按月、季度、半年或年结算，年化趋势按自然月复合，四种周期独立配置扰动；利润由收入、费用和税务推导。
- 分红方案、法定事实、结果与事实类型已上移到 `company::dividend` 共同契约，Session 不再依赖 `simple::` 私有类型；这只完成类型面，查询、能力与完整公司行为契约仍未完成。
- `CompanyKind` 从配置显式传入，发行人、财务 owner 与科目表严格一致。四类基础科目表上的五个 `simple_*` 科目仅表示虚拟汇总，不冒充贷款、保费、赔付或真实公司收付款。
- 新局可编辑虚拟 preset；允许开局股价、总股本及虚拟 PE／PB 倍率的一次性初始化校准。预览和创建消费同份 seed／配置，后续结算与恢复不重新锚定，不保证开盘无涨跌。
- 报告来自实际已结算期间，未公开材料不泄漏；长周期不能伪造短周期资料。月末更正、所得税和公开版本使用候选事务，失败不留下部分过账。
- 浏览器本地日终使用 IndexedDB，Native 使用 SQLite；共享市场授权与经济账户分开。日内不保存挂单，不启用 WAL。

## 验证范围

只做定向短单测、独立复核与编译／构建，没有运行完整回归。

| 验证 | 实际结果 | 证据 |
| --- | --- | --- |
| Simple、周期、定点、Finance、披露、Session、更正、集团基础及股本 codec 九组 | 85 项通过；每组外部 deadline 为 10000ms，组间并发，组内 8 线程 | `.tmp/checklist-wave4/host70-simple-short.log` |
| Web 新合同、初始化与真实日终档 | 45 项通过；4 并发，case 和命令均 10000ms | `.tmp/company-system/web-final-kind-fresh-green.log` |
| 月末后置故障回滚与修复重试、事件映射、独立 scheduler、工业税／折旧、地产零税 | 5 项通过；外部 deadline 为 10000ms，并发执行 | `.tmp/checklist-wave4/host76-migration-short-tests.log` |
| CompanyPanel 公司／时间线归属隔离 | 7 项通过，正式外部 deadline 为 10000ms | `.tmp/checklist-wave4/host77-panel-owner-pair.log` |
| ts-rs 当前绑定 | 128 项导出通过，未手写 generated | `.tmp/checklist-wave4/host70-types-generate.log` |
| 默认 features 的 workspace 全部 targets | 编译检查通过，Cargo jobs=32，不执行这些测试 | `.tmp/checklist-wave4/host75-workspace-consumers-check.stderr` |
| Web 类型检查 | 强制重新检查通过，日志保留命令、deadline 与 exit code | `.tmp/checklist-wave4/host79-types-evidence.log` |
| WASM 多线程与 Web release 构建 | 通过，jobs=32，共享 300000ms deadline | `.tmp/checklist-wave4/host75-frontend-build-direct-pnpm.log` |
| 面板最后修复后的 Web 成品 | 类型检查、Vite 构建与 release WASM 产物检查再次通过 | `.tmp/checklist-wave4/host78-final-web-build.log` |
| 公司行为基础 | 整数分纯现金除息 9 项、分红方案与登记／付款结果事实 14 项通过；各命令外部 10000ms deadline、非作者限定复核通过 | `.tmp/company-system/ex-reference-short.log`、`.tmp/company-system/checklist-common/cash-final-green.log` |
| 期间时间加权权益分析基础 | basis 错配先产生真实失败，修复后 7 项通过；显式区分报告范围和权益归属 | `.tmp/company-system/checklist-common/basis-red.log`、`.tmp/company-system/checklist-common/roe-fresh-green.log` |
| 现金贷记、披露 ROE 一般公式与股份登记精度 | Account 5 项、法定 ROE 一般公式 13 项、股份登记含 i128 边界 15 项通过；非作者限定复核通过 | `.tmp/company-system/session-actions/fresh-0.log`、`fresh-1.log`、`review-fix-0.log` |
| 开局股价反推虚拟基本面与预览消费 | 14 项通过，覆盖四周期、seed 边界、PE/PB 舍入勾稽、编辑保留；不保证开盘无涨跌 | `.tmp/company-system/session-actions/seed-price-confirmation.log` |
| 现金分红候选接线与个人公开市场股息税 | Finance 18 项、Session Simple 14 项通过；正规 typegen 135 项通过。后续真实卖出补税两个 Session 短测、Web 公司行为 parser 13 项、当前存档契约 33 项和 TypeScript 检查通过。宿主/UI 配置入口仍缺，企业、基金、非居民身份显式不支持 | `.tmp/company-system/session-actions/connected-0.log`、`connected-1.log`、`typegen-review-fix.log` 及本轮终端复测 |
| 报告窗口及公开披露 ROE 接线 | 报告期间 10 项、公式适配 3 项、官方一般公式 13 项、合并窗口 10 项、公开投影 5 项通过；新类型正规导出 140 项通过 | `.tmp/company-system/session-actions/final-period.log`、`final-report-adapter.log`、`latest-official-roe.log`、`final-consolidated.log`、`public-projection-assert-green.log`、`latest-typegen-shard-*.log` |
| 现金分红公开公告与本人信息不足 | Session Simple 16 项、信息交付 10 项、中期材料及到期 1 项通过；合法缺年报不阻断中期获知，损坏引用仍回滚 | `.tmp/company-system/session-actions/final-announcements.log`、`legal-monthly-final-green.log`、`final-interim.log` |
| 送转分配基础与回购专户来源 | Registry 21 项、分配算法 9 项、Web 严格来源解析 11 项通过；尚非实际送转结算 | `.tmp/company-system/session-actions/final-registry.log`、`final-allocation.log`、`latest-web-registry-green.log` |

本机旧 Corepack 直接启动 pnpm 时失败，原日志保留于 `.tmp/checklist-wave4/host75-frontend-build.log`；实际构建使用 Node 启动已缓存的同版 pnpm 11.19.0，之后仍运行原 WASM／Web 构建和发布产物检查步骤。没有修改系统工具或用失败结果冒充通过。

手工 `currentSaveFixture` 已迁移为当前 release Engine 正规生成的最小日终档；主档、休市档与最小档均由真实创建、日结和恢复／重存验证生成。三份完整新契约档经 Web `parseSaveSlot` 解析后与原 JSON 深度相等，证据为 `.tmp/company-system/session-actions/final-generated-three-strict.log`；不做兼容填充或手补 JSON。早先缺 `company_system` 的失败日志 `legacy-fixture-contract-limit.log` 保留作为迁移前证据，不代表当前档状态。独立手写 schema fixture 的迁移与各消费者短测仍按[迁移记录](current-save-fixture-migration.md)核验，不能以完整档解析替代全部消费者验证。共享 Protocol fixture 的 MarketSnap 新字段遗漏已单独补齐，`protocol-parse` 与 `protocol-runtime-store` 重跑 14 项通过，证据为 `.tmp/company-system/session-actions/protocol-anchor-verified.log`。

主存档与休市存档由 release Engine 正规生成，验证真实恢复深等；主场景另验证恢复前后继续三个 frame 都产生真实 NPC 受理。没有手补 JSON。旧经济轨迹的密封 hash、长期回归、Windows／macOS runtime 及完整浏览器验收不在本批通过范围。

## 未完成边界

共同股本行为仍未完整交付。当前候选接线已在显式登记名册、法定事实与获批现金方案下验证 Session 真实税前到账、公司应付清偿及同日合计除息锚；失败不提交部分状态，重复执行不重复到账或扣减除息参考价。公开 typed 分红公告、NPC 本人获知和公告／批准财务金额恢复勾稽已接通；个人公开市场税务身份可在 Session 显式配置，税账记录每笔分红与真实 FIFO 卖出，卖出不足一个月时用真实卖出净额收缴 20% 补税，恢复和 Web 严格 parser 校验同一税账事实。宿主/UI 配置入口仍缺，企业、基金、非居民身份显式不支持，未配置身份仍为 `TreatmentNotConfigured`。默认开局名册与税务身份未定，公司行为偏好、自动方案、送转实际登记、配股、增发和回购仍未完成，`cash_settlement=false` 不因个人税或底层测试通过而改为完整可用。获批特殊除息调整仍明确拒绝，基础计算及候选接线分别见[除息基础复核](ex-reference-price-review.md)、[Session 接线复核](session-corporate-actions-review.md)。

`cash_dividend` 提供明确方案、不可变登记权利与付款结果状态机，连续失败、技术重试和严格 JSON 恢复有短测；状态机本身只记录付款执行者返回的事实，实际账户贷记由 Session 候选事务执行。恢复显式提供日历调用 `validate_with_calendar`，不默认补齐日历，并校验 Book、公司应付凭证、实际到账及除息锚的跨域对应，详见[分红基础复核](cash-dividend-book-review.md)与[财务接线复核](simple-dividend-finance-review.md)。`accounting::period_roe` 仍是同范围、同权益归属的时间加权分析基础，不冒充法定披露 ROE；`accounting::disclosure_roe` 一般公式已进入真实报告窗口、公开 DTO 与 Web 报表消费。同一控制合并／比较期间特殊处理、合并归属逐项权益事件及扣非事实仍明确不可用，详见[期间权益分析复核](period-roe-review.md)、[法定一般公式复核](official-roe-review.md)与[报告接线复核](simple-report-roe-review.md)。合法本人中期资料缺同范围年报及其到期不再阻断日结；损坏材料引用仍失败回滚。合法信用违约公告缺本人年报时会记录年度基线不可用并保留公告因果；非正基本面估值区间对候选信号表现为 `FundamentalUnavailable`，不伪装成 0 元，见[个人信息边界审计](missing-annual-belief-audit.md)。

完整经营、客户有限现金和行业业务由后续 `company/simulation/` 分支实现。旧集团／冲击经营 Session 的特定集成原文已归档；现有独立账务用例不等于这些旧接线仍存在或全部已验收。具体状态按[实施清单](implementation-checklist.md)和[历史审计总账](../implementation-audit/implementation-audit-2026-10-02.md)继续推进，不从归档或编译成功推断全部功能完成。
