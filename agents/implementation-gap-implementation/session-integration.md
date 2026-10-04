# Session 入口整合记录

## 范围与分工

- 本轮仅实施 `packages/engine/src/session.rs` 与 `packages/engine/src/session/persistence.rs` 的共享入口接线；本记录不替代正式规范，不执行 stage 或 commit。
- 依据：AGENTS、principles、architecture、open-questions，以及既有公司域/个人信息与经历 ADR 契约。没有更改 A 股撮合、T+1、股东侧现金流或交易单位规则。
- 业务模块的失败测试及短 fixture 由各缺口 owner 提供，入口整合保持既有测试断言与 fixture 设定，仅为新增 setup 字段补 `None` / 空 groups。

## Hunk 与缺口映射

| 缺口 | 入口接线 |
| --- | --- |
| G07 | Retail 初始化 BeliefBook，不设置 Institution policy；Institution holdings reconciliation 过滤 Institution profile；restore 校验 profile 与账户类别，禁止 Retail 携带 Institution policy |
| G08 | 开局持仓调用 initialize_holding_dated，保存 civil_date、market_minute、trading_day |
| G16 | closing/library 使用 Arc；new/restore 创建 Arc，save 提取底层事实，mutation 使用 Arc::make_mut，shadow 共享不可变历史 |
| G28/G36 | company_groups 注册与公开类型；setup/save/state groups；新局及 restore 校验；custom unlisted 公司合法，完整公司 keys 与 setup 重建 spec 一致；月末 group closing、day-end disclosure、shadow/save/restore 接线 |
| G35/G41 | restore 校验 payment history；ProjectLoan maturity 按存档 CalendarPolicy 的初始化下界与运行上界验证，允许合法逾期贷款 |
| G42 | watchlist/price memory 仅对未持仓且无 active plan 的股票计数，严格限制 8 个 |
| G73 | 对直接内存 SaveSlot 的 InsuranceBooks 执行 validate_restore |

## 验证与复核

- 编译参数：共享 `flock /tmp/stock-market-gap-cargo.lock`，`timeout 300s cargo test -p engine --lib --no-run --target-dir .tmp/gap-target -j16`。首轮发现 hash destructuring 缺 groups，已转交 hash owner 修复。
- 普通测试只直接运行生成的 test binary，使用 `timeout 10s` 与 `--test-threads=8`；不执行完整回归。
- 沙箱内 `ps` 只可见当前调用的进程，不能据此声称观察到其他构建进程的真实 CPU 利用率；显式使用 `-j16`，CPU 取证需可见宿主进程树的 owner 完成。
- G16 入口已通知独立 reviewer；其余入口等待 root 安排非作者复核。当前不宣称所有缺口通过最终门禁。
- 第二轮 `--lib --no-run -j16` 成功，编译耗时 53.76 秒；生成 `engine-6ae5d5d49d140673`。
- 原命令 `flock timeout` 未把等待锁纳入 deadline；已向 root 承认并修正后续约定为 `timeout 300s flock ... cargo ... -j16`，本轮不复用旧错误命令。
- 定向执行 Retail analysis 四例：3 通过，1 失败。失败为 public-step replay fixture 把 attention probability 改为 1，却未同步 StrategyState probability，触发现有严格一致性校验；已交 G07 owner 修 fixture，不删除或弱化断言。
- 两例 Institution 存档边界测试通过：policy latch 必填/严格类型/恢复保留；confidence 0..10000 接受且超上限拒绝。分别 0.56 秒与 0.96 秒，8 test threads，10 秒 deadline。
- 后续 G16 reviewer 发现同名 binary 被 isolated 构建覆盖（用例总量从 1096 变成 1083）；上述执行记录只保留当时实际输出，不作为最新完整源码的最终验证。已告知 root 隔离 target 并统一重新发布共享 binary，不把筛选到 0 个测试当作通过。
- 经营独立复核补充发现已修：payment failure 历史日期须落在存档 CalendarPolicy 的初始化下界至运行上界；active shock 必须适用于公司行业。动态边界测试待业务 owner 提供，并再次复核。

## 后续复核增量

- G07：Web 与 Rust 对 Institution 专用 risk latch 存在漂移；Rust 增加拒绝非 Institution 的 `institution_account_risk_paused=true`，新增 `retail_restore_rejects_institution_account_risk_pause` 短测试。先添加测试，再实现 guard；未独立编译执行 red 阶段，等待 root 单次共享构建，不能声称已取得失败运行证据。
- G28：直接内存 SaveSlot 对所有 IndustrialBooks 调用 `validate_trade_counterparty_events` 与 `validate_inventory_source_events`，与 JSON serde 的事实校验一致；包括未纳入 groups 的独立公司。
- G42：成功日结事务在 `record_civil_day_events` 之后调用 `prune_all_personal_memories`，保持持仓与 active plans 保护；调用处仍在日结 checkpoint 的 rollback 范围内，不引入逐 tick 全账户扫描。
- 上述增量 `git diff --check` 通过；已交业务 owner 与独立 reviewer，再次编译/短测由 root 统一发布新 binary，避免重复构建半成品。
- G28/G36 后续 root 授权的必要 scope 修正：`validate_company_domain` 调用 ClosingEngine 的 `validate_consolidated_parent_income`，仅检查 Consolidated 报告归母净利润必填，覆盖 direct typed SaveSlot；不声称全面修复 Q17 closing registry 校验。helper 与测试由 company assembly owner 实施，等待其接口 ready 后共享构建。
- helper ready 后补充 `direct_save_slot_rejects_missing_consolidated_parent_income`：正常 SaveSlot restore 成功为正控，替换为生产源 cfg(test) helper 构造的内存坏 ClosingEngine 为负控，不经过 serde；要求明确 InvalidSave 且同时包含 closing registry 与 income.net_income_to_parent。已交独立 company assembly reviewer 执行精确短测试。
