# 四行业装配与固定集团公开链

## 范围与边界

- 本轮处理 G36 的自定义四行业 Session 装配、实际封账与公共报告查询，以及 G28 的固定集团装配、历史 source 身份、合并五产物与公开查询。
- 行业 `books_mut`、经营到期与风险公告由经营 owner 负责；`session.rs` / `persistence.rs` 由共享契约 owner 接入；Web strict schema 由 Web owner 接入。没有改总账、README、Git index，没有提交。
- 依赖现行 ADR-0016/K3 任务 12/13/26、`docs/company-accounting.md` 的 CAS 33 法源和明确游戏假设；不新增行情、股东融资/分红、投资者补钱或真实会计合规声明。
- 默认公司仍为 Industrial；自定义 `CompanyOperationsConfig` 要求前史首日前一天的单张 `OpeningBalance`，随后走真实经营前史。固定持股事实保存于 `groups`，严格当前存档，不迁移 legacy。
- 内部往来 source 身份与库存 source 身份只保存归属；金额、日期、方向从真实 Journal 读取。当前原始分录与历史已公布报告保持不变，工作底稿不回写成员账套。
- 正式装配边界、非 FIFO 的均匀成本池分摊及显式不支持项登记于 `docs/company-accounting.md` §8 和 `docs/trading-rules.md`，不把不可辨识内部销售静默转换为空抵销。

## 实施链

1. `company_assembly` 从显式四行业 books/flow/spec 装配发行人注册表，校验股票映射和股本，播种原始前史报告及固定集团报告。
2. 月末真实单体 `close_month` / `close_year` 后登记集团月报及对应中期/年度快照；排期披露沿根公司发布独立 `Consolidated` ID，公共 query 同时保留单体与集团。
3. `TradeCounterpartyEvent` 和 `InventorySourceEvent` 为历史余额、内部赊销与库存成本池提供必要 source 身份。恢复同时验证存在性、重复和实际 Journal / InventoryLedger 的完整身份覆盖。
4. 往来金额既验证单对上界，又按 `(member, account)` 验证不同成员对的累计上界。历史截止后结付仍不会抹掉截止时点的 AR/AP。
5. 同码异义科目使用版本化投影 key，分类逐成员来源建立；报表只扫描截止期间内分录。利润表少数/归母使用当年损益，权益变动使用报告窗口，权益余额保留历史累计。

## 红绿证据

- 最初两个新增短测动态为红：工商/保险 1122 发生 `DuplicateClassification`；零实际余额的相等 AR/AP 申报没有走真实上界错误。
- 真实混合集团 Session 初始化进一步动态为红：前史累计净利被误用为当年净利，输出 `derived 44732.01 vs task-12 output 188103.24`；修复窗口拆分后不再发生。
- 内部真实单批 `purchase` / `sell_credit` / 部分外售 / 后续 `collect` / `settle_payable`，历史 source 截止与恢复均消费生产 `ensure_group_report`。手算：March 内部 AR/AP 各 226 元、未售转移成本 100 元、未实现利润 50 元、集团净利 110 元、少数损益 12 元、少数权益 212 元、经营 CF −226 元、现金 1774 元、存货 150 元。
- 上述手算第一次写错 assets 2110 元。现有 VAT 分类净借 5.20 元呈负 TaxesPayable，而不是独立 VAT 资产；正确 assets 2104.80 元、liabilities −5.20 元、equity 2110 元。本轮仅纠正手算锚，不改现有税务列报，也不删除/放宽断言。
- 新真实 Session 内部交易 fixture 保留原经营前史 Journal，再经真实行业 API 追加交易，以受支持的可编辑 SaveSlot 导入，恢复后真实月末封账、再次恢复继续日结至排期，公共 query 取出五产物与集团 ID；不是 `books_mut` 或重复实现报告算法。

## 短验证

- 编译准备：`flock /tmp/stock-market-gap-cargo.lock env CARGO_TARGET_DIR=.tmp/gap-target cargo test -p engine --lib --test session_company_assembly --test consolidation --test industry_reports --no-run -j16`；未运行全回归或长矩阵。
- 普通测试：各二进制均由 `timeout 10s` 限制进程树，`--test-threads=8`。`industry_reports` 19/19，`consolidation` 27/27，历史内部交易专用 lib filter 1/1；各在 0.02 秒内。
- 独立 reviewer 已运行真实 Session 三 case：四行业封月/query/restore 继续日结、混合集团前史与日结披露/restore、真实内部单批及部分外售的 Session/save/public-query；独立版本 3/3、7.34 秒。最终补充 final-save restore 后继续日结与原公开 ID 仍可查的版本，实施方重跑 3/3、7.19 秒。
- 三个最新实际 exporter 输出为 `.tmp/company-assembly-export-final/four-industries.json`、`.tmp/company-assembly-export-final/mixed-group.json`、`.tmp/company-assembly-export-final/internal-sale-group.json`。Web owner 最终对实施方与 reviewer 两目录共六份真实 JSON 执行 `parseSaveSlot` + 原值深比较，以及分别删除 `inventory_source_events` 的负控；六份正控与六份负控均绿，共享 10 秒 deadline 内实际 1.65 秒。不以旧 schema projection fixture 冒充合法 Rust 新档。
- 独立复核 owner 为 `review_company_assembly`；其发现的跨年净利、累计上界、当前余额污染历史、内部销售漏抵销、库存身份遗漏与未来 item 污染历史截止均已落实修复与新增断言。最新独立 Session 3/3、7.14 秒。经营 owner 的真实 Bank/Insurance 非空公告 2/2 被 reviewer 独立重跑通过，1.26 秒；G28/G36 联合最终 PASS，不把本文件的 quiet fixture 当作公告证据。

## 归母损益源必填增量（用户明确 A 股 scope 后）

- `Consolidated` 的 `income.net_income_to_parent` 必须存在，缺失返回类型化 `ReportError::MissingParentIncome { scope, period }`。`Standalone` 合法 `None` 与合并合法 `Some(0)` 保持接受，不用集团净利润或 0 冒充归母净利润。
- `ReportSet::validate_parent_income_source` 是共用单条件；报告完整 validate、closing source helper、公开库形状校验都复用这一来源约束。
- `ClosingEngine` serde 在原始 versions 转换为 keyed map 之前逐报告检查该条件，避免早期坏报告被后来合法同 key 行覆盖而静默吞掉。directtyped `SaveSlot` 的同条件恢复检查由共享 persistence owner 接入。
- 不扩大为 Q17 的完整 `ClosingEngine` 恢复校验；版本键上下文、完整重述及其他恢复不变量仍保持原范围。
- 原始来源反例先动态红：删合并归母净利后 `ReportSet::validate` 仍返回 `Ok(())`，同批 `Standalone None` 正控通过。随后 closing serde 单坏版本反例也动态红，返回含缺失归母来源的 `ClosingEngine`，process exit 101；later 同 key 合法行的负控与之同测。
- 最后 `--list` 明确列出四个 `closing_restore_` case 与 directtyped case 后，native closing 4/4、0.00 秒，`session::persistence::direct_save_slot_rejects_missing_consolidated_parent_income` 1/1、0.37 秒，`industry_reports` 22/22、0.02 秒。各命令均使用 `timeout 10s`、`--test-threads=8`，编译准备共享 lock 和 `-j16`；没有运行全回归。原独立 reviewer 随后重跑 4/4、0.00 秒，typed case 1/1、0.39 秒，行业报告 22/22、0.01 秒，完整增量静审与 diff 检查通过，归母来源及窄 closing 恢复增量最终 PASS；Q17 其余边界不核销，不以 stale binary 或 0 case 冒充验证。
