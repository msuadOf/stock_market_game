# Q14 Bank 真实贷款来源独立复核

## 范围与边界

reviewer 未实施本批，读取 Bank `claim_sources.rs`、`claim_source_tests.rs`、`mod.rs`、`loans.rs`、`lending.rs`、`interest.rs` 完整本批 diff，及 Web `bank.ts`、贷款来源 fixture／测试、相邻 fixture／测试增量和工作记录。`income_tax.rs` 仅新增 `validate_loan_claim_sources` 调用归本批，其余税务 hunk 属其他任务；不因本次审查审批并行税务代码。本轮没有启动 Cargo 或提交。

来源记录仅保存真实 loan／source／kind，日期和金额仍取唯一 Journal；本金捕获实际发放事件，每笔正额利息结果携带真正创建的 source，零额不造来源或凭证。start／maturity 从原输入持久保存，不设置默认期限，不修改 LN 到期先息后本、ACT/365F 及逾期继续计息。原子性改善将 `apply_accrual` 的 checked 更新提前在私有状态完成，再安装 Books 和所有子账；没有据金额、Journal 位置或字典序猜 loan。范围必要，符合 Q14 已确认经营现金与来源边界，但尚不代表客户有限现金或生产收付闭环接入。

## 有效发现

### B1：恢复事件游标可以落入已过账来源

新增 `BankBooks::Deserialize` 结尾只执行来源覆盖校验，未校验 `next_event_id` 大于已有真实 Journal 来源。已有 Owner 校验入口包含该守卫，但不能代替直接 Bank 恢复边界；Web 同样只验证游标为非负整数。合法账簿把游标改为本金或利息实际 source 时，来源覆盖仍完全成立，恢复后的下一条实际操作会撞到已过账身份。

应在本批来源恢复校验及 Web 对应入口检查 cursor，不必为了此项调用完整税务 Owner 校验而改变其他测试约定。补合法 fixture 篡改 cursor 的拒绝短测，以及高于已过账来源的合法空洞 cursor 仍允许。

### B2：Web 收集来源前会折叠重复 Journal

Web `validateClaimSources` 将全部 Journal 直接构造成 `Map`，重复 source 被 last-wins 覆盖后才求 expected 与来源覆盖，复制完整真实本金分录仍可能通过。Rust 底座 `Books::Deserialize` 逐批 `post_batch`，明确拒绝重复来源，已亲读该路径，不能把 Web 的折叠视为 Rust 同样接受。

应在 Bank Web 来源校验构建索引时显式拒绝重复 Journal source，保持本批局部范围，不必全局扩张 Journal parser。补复制本金凭证与同 source 不同 kind 负例；Rust 增重复 Journal guard 用于证明跨层一致。

### B3：贷款子账未知字段跨层不一致

Web `parseLoan` 对当前贷款字段 exact 检查；Rust `BankLoanState` 仍默认 derive Deserialize，未知字段会被忽略。新增期限持久化与严格恢复宣称不应形成此漂移。

为贷款状态补 `deny_unknown_fields` 并加合法 JSON 未知字段拒绝 case，必填期限无默认补齐，不增加旧档兼容。

## 当前门禁

**本批限定 PASS。** 二轮已完整读取修复源码与 8 项 Rust 测试、6 项 Web 来源测试：B1 在 Rust 来源校验中检查全 Journal 来源必须小于 cursor，Web 同样覆盖全部 Journal，空贷款也不绕过开局来源，允许合法非连续高游标；B2 Web 以 Journal 数量与 Map 大小不等明确拒绝重复，测试涵盖重复本金及同 source 异 kind，Rust 通过既有 Books 重放拒绝并补 guard；B3 贷款 serde 已加 `deny_unknown_fields`，与 Web exact 一致。新增逾期恢复、来源保存及失败发放／倒退计息／序号溢出／重复过账原子性断言保持必要边界。未发现新的代码阻断，未放宽断言或增加兼容。

最终 reviewer 亲读 `.tmp/checklist-wave4/q14-bank-source-host46/` 的结果和全部 8 份精确 case 日志：6 项通过，游标和未知字段 2 项业务断言真实失败，非编译错误或零 case。随后亲读 `q14-bank-source-host47/` 的结果及全部 8 份日志，确认各项实际运行 1 case、1 passed、0 failed，8/8 通过，case 用时 0.00–0.01 秒。另 reviewer 独立运行 Web 六项来源短测：外部 `run-with-deadline.mjs 10000`、Node case 10000ms、`--test-concurrency=8`，6/6 通过，整命令约 0.30 秒，没有运行 Cargo 或完整回归。已完整读取融合更新的工作记录，部分构建失败与后续实测绿及生产未接线口径准确。

该批结论仅限真实来源准备及上述测试边界，不覆盖 CompanyOperations 客户付款、另一侧 Books 原子入账或经济默认参数，也不将 Bank 直接 serde 的局部来源校验冒称完整税务 Owner 校验。
