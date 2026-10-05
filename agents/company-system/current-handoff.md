# CompanySystem 当前交接

## 已接通的代码

`packages/engine/src/company/` 是共同入口，`simple/` 已独立承接基本面生成与汇总财务；`GameSession` 不再运行旧 `CompanyOperations` 后台。`Simulation` 当前明确拒绝创建，后续由用户另开分支实现，不做默认降级、格式兼容或 schema 迁移。

- 按月、季度、半年或年结算，年化趋势按自然月复合，四种周期独立配置扰动；利润由收入、费用和税务推导。
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

本机旧 Corepack 直接启动 pnpm 时失败，原日志保留于 `.tmp/checklist-wave4/host75-frontend-build.log`；实际构建使用 Node 启动已缓存的同版 pnpm 11.19.0，之后仍运行原 WASM／Web 构建和发布产物检查步骤。没有修改系统工具或用失败结果冒充通过。

主存档与休市存档由 release Engine 正规生成，验证真实恢复深等；主场景另验证恢复前后继续三个 frame 都产生真实 NPC 受理。没有手补 JSON。旧经济轨迹的密封 hash、长期回归、Windows／macOS runtime 及完整浏览器验收不在本批通过范围。

## 未完成边界

共同股本行为的实际投资者结算、公司行为偏好与自动方案尚未接通。登记和个人股息税底层有独立短测，但不能当作分红、送转、认购或回购已能从 Session 执行；当前 `cash_settlement=false` 明示此缺口。

完整经营、客户有限现金和行业业务由后续 `company/simulation/` 分支实现。旧集团／冲击经营 Session 的特定集成原文已归档；现有独立账务用例不等于这些旧接线仍存在或全部已验收。具体状态按[实施清单](implementation-checklist.md)和[历史审计总账](../implementation-audit/implementation-audit-2026-10-02.md)继续推进，不从归档或编译成功推断全部功能完成。
