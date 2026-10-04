# Web 存档契约接线记录

## 范围与归属

- G28/G36：`schema/market.ts` 的可省略 `SessionSetup.company_operations` / `groups`，`company/operations.ts` 的 `CompanyOperationsConfig` 严格类型与解析，新增 `company/groups.ts`，`schema/root.ts` 的必填 `SaveSlot.groups`，以及相应测试、fixture 与两份生成 binding。
- G28：`company/books/industrial.ts` 新增必填 `trade_counterparty_events`，只保存 `event` / `counterparty` / `account` 身份，不伪造金额；对应 `company-books-schema.test.ts` 和两个 JSON fixture。
- G28 追加：同一 IndustrialBooks 新增必填 `inventory_source_events`，严格保存 `event` / `item` / `account`，拒绝额外金额或数量。Journal / InventoryLedger 的深度引用关联由 Rust authority 校验；Web 没有假装从旧存档恢复遗失身份。
- G35/G41：`company/operations.ts` 的必填 `payment_failures`，`company/policies/shock.ts` 的正 `i128` 分金额与非空事项、active 排除，`company/reports.ts` 公告共享同一 `ShockKind` parser 并强制单日、幅度零；`company/books/real-estate.ts` 的必填 `maturity_date` 保留真实逾期贷款，不能误加必须晚于 accrual 的约束。
- G07：`personal/beliefs.ts` 与 `.test.ts` 保留 Retail 的已有 BeliefBook shape，要求 `institution_policy=null` 和 `institution_account_risk_paused=false`；不要求 RetailStyle 与账户策略逐项全等，不擅自裁定 Q25。
- 新增 `company-contracts.test.ts` 覆盖 setup 省略与显式 null、空 groups、严格 config、group 重复与安全股份、PaymentFailure、公告和支付历史边界。

## 依据与分层

已阅读项目 AGENTS、principles、architecture、open-questions 与 ADR-0016；依据 Rust owner 确认的真实 serde shape 接线，不重新决定交易制度。公司经营金额为两位小数的会计金额，支付失败不得直接模拟报价方向，也不得把尚未支付事项当作已支付现金。公司经营与 NPC 个人资产仍隔离。

Web 负责严格结构、标量及新增事实的直接边界；集团控制比例、公司/spec/行业账套/flow 一致性、OpeningBalance 日期、历史 Journal 与 counterparty 的深度关联仍由 Rust 原子 restore / setup authority 校验，不把 TypeScript parser 的通过冒称 Rust 恢复通过。

两个旧 JSON fixture 只补显式新字段（`groups=[]`、`payment_failures={}`、`trade_counterparty_events=[]`），作为既有 schema projection 的测试输入；不声称补出了旧 fixture 遗失的交易身份，也不声称它们是新 Rust 生成的合法集团存档。真实四行业/集团输出的动态 bridge 证据待 Rust exporter 提供。

## TDD 与验证

1. 新 setup、PaymentFailure、payment_failures 三 case 先红（3/3 失败），再实现严格 parser 后绿。
2. 公开 PaymentFailure 与 Retail Institution 边界先红（2 case），实现后 10/10 绿。
3. 新 `trade_counterparty_events` case 先红（未知字段），实现后相关 12/12 绿。
4. RealEstate 既有 populated fixture 随新增必填字段更新，并补逾期、缺失和非法日期 case；该补测不是单独的先红步骤，不能冒称所有新增 case 都执行了红阶段。
5. 定向八文件第一次 54/54 绿；增补 group case 后四个独立 Node 进程并发 4 分片 55/55 绿，命令约 0.34 秒；case `--test-timeout=10000`，外层共享 `timeout --kill-after=1s 10s`。未执行完整回归。
6. `tsc -b apps/web/tsconfig.app.json --pretty false` 约 6.2 秒通过；`git diff --check` 通过。
7. 完成 RealEstate 与 Industrial 新边界后，同八文件四个 Node 进程分片最终 57/57 通过；再次 `tsc -b` 和 `git diff --check` 通过。

最初 Node 默认 process isolation 在当前环境产生无诊断的 child failure；使用仓库同款 `--test-isolation=none` 后才获得明确失败原因与有效测试结果，未把首次启动失败当成业务红阶段。

## 生成 binding 与未完成门禁

使用 session owner 提供的既有 engine binary 精确执行 `session::export_bindings_sessionsetup` / `session::export_bindings_saveslot` 两项导出，并发 2，不执行全量。直接 binary 未继承 Cargo 的 `TS_RS_EXPORT_DIR`，产物落于 `packages/engine/bindings`；仅将实际 ts-rs 生成的 `SessionSetup.ts` / `SaveSlot.ts` 用 `apply_patch` 接入正式目录，没有手造 unknown 接口。

两项导出共享十秒命令到期：SessionSetup 9.31 秒通过，SaveSlot 已发布产物但测试未报告完成，命令退出 124。因此不能宣称导出测试全绿；最终仍须 root 用确认身份的完整 binary 重新生成并验证。当前文件的 TypeScript 编译通过不替代这个门禁。

G28/G36 reviewer 已独立检查 strict shape 与增量 trade 字段；G07/G35/G41 独立 reviewer 正在检查完整跨层 diff。首轮确认 Retail risk pause 的 Rust restore 边界遗漏，已交唯一 owner 修复并待再次复核。未 stage、未 commit、未修改审计总账。

## 追加来源身份与真实 bridge

`inventory_source_events` 新 case 先因未知字段失败，再实现严格 parser 后同公司账套/完整存档两个套件 13/13 通过（约 0.38 秒）。公司独立 reviewer 已静审追加 shape。

在追加该字段前，对独立 reviewer 导出的 `.tmp/company-assembly-review/four-industries.json` 与 `mixed-group.json` 执行真实 `parseSaveSlot` 并 `assert.deepEqual` 对比全部原值，两项通过（约 0.53 秒）。这不是手造 fixture，但仅证明当时的契约；新增必填来源字段后不复用旧证据，等待最新四行业、集团与内部库存转移三份真实输出重新过桥。

2026-10-04 最终追加：公司独立 reviewer 用新版真实 Session 测试生成 `.tmp/company-assembly-review-v2/{four-industries,mixed-group,internal-sale-group}.json`。三份均通过 `parseSaveSlot` 与 `assert.deepEqual` 全量原值保留；逐份删除真实 Industrial 的 `inventory_source_events`，三项负控均被明确拒绝。外层共享 deadline 十秒，命令约 0.74 秒。此证据替代上一轮旧字段桥接，不替代长期会计验收。

G07 Rust owner 已追加非 Institution 不得携带 `institution_account_risk_paused=true` 的恢复校验及真实 Retail 负控测试；Web 独立 reviewer 已再次静态复核通过，报告见 `web-save-contracts-review.md`。未重复构建，不把静态复核冒称新增 Rust case 动态通过；动态门禁由 root 统一核收。

## 最终 binding 门禁

2026-10-04 16:25:17 更新的 `.tmp/gap-target/debug/deps/engine-6ae5d5d49d140673` 已由 root 的共享构建及公司 reviewer 确认；`--list` 精确找到两个导出测试。将 `TS_RS_EXPORT_DIR` 显式设置为两个彼此隔离的 `.tmp/web-save-binding-final/{setup,save}` 目录，并发两个进程、各 `--test-threads=4`，共享十秒 deadline，两个精确 case 均通过（约 0.63 秒）。正式 `SessionSetup.ts` 与 `SaveSlot.ts` 分别与新生成产物 `cmp` 逐字节一致。

该成功取代首次超时的未完成生成门禁；首次失败仍保留历史，不改写为通过。最终 app TypeScript 编译约 6.2 秒通过，`git diff --check` 通过。

最终 Session 测试追加最终存档恢复并继续日结断言后，再对 reviewer 的 `.tmp/company-assembly-review-final/` 和实施者的 `.tmp/company-assembly-export-final/` 各三份真实输出重新过桥。六份均 `parseSaveSlot` + `assert.deepEqual` 原值保留通过，六份各删除 `inventory_source_events` 的负控均明确拒绝；共享十秒 deadline 内结束。此轮绑定最终测试输出，无产品改动或重复回归。
