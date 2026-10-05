# 独立财务恢复测试迁移

## 领域与分层

本轮生产 Session 只运行 `CompanySystem + Simple`，不保留隐藏 `CompanyOperations`、Books 或账务日结。
已有独立工商、银行、保险账务规则仍按原始单位与校验保留，不扩成完整 Simulation 生产路径。

## 迁移范围

- `save_contract/bank_policy.rs`：四类非法 ECL policy 改为真实独立 Bank 装配后的 `CompanyOperations` 往返；保留 ECL 与公司身份错误断言。
- `save_contract/restore_guards.rs`：负贷款、组合金额溢出、重复 scheduler due id 改为独立经营状态解码，原错误原因断言保留。
  Simple Session 的 plan horizon、CivilClock 重复 id／越界日期测试仍留在公共恢复入口，合法 due 通过 `register_due` 明确构造。
- `scale_restore_limits.rs`：257 公司无配额改为真实 Simple CompanySystem 往返，并保留重复发行映射拒绝；
  普通 Session 额外验证同 issuer ID 的总股本身份不匹配，避免用旧账务公司集合代替当前契约。
- `insurance/session_restore_tests.rs`：完整 Session 的合法保险子账往返与 in-memory GMM 损坏恢复守卫迁为独立 `CompanyOperations`，
  保留公司、合同组、GMM 原因及未修改合法基线的断言。

## TDD 状态

Bank ECL 已移至 `tests/financial_restore.rs` 独立入口，原 seed 与四类错误断言不变。
root 用 host58 真实生产 Engine library 编译该原测试源后，实际 case 因“损坏 ECL 政策被接受”失败：
`.tmp/checklist-wave4/host58-financial-restore-red.log`。此前 whole-lib 粗边不是有效红测。

取得实际红后，原纯 owner 校验迁至 `company/operations/restore.rs`：公开 `validate_restored` 保留 ECL、
工商 credit／trade／inventory、各行业 income-tax／owner 与保险 GMM 守卫，并保留公司身份和原错误 source。
独立 Operations 严格 Deserialize 调用该校验；不挂 Simple Session，不启用后台 Books。
nullable 事实字段仍必填；唯一重建的是未持久化的派生 hash cache，不增加兼容或字段补齐。

root host60 真实当前产物已执行两个关键 case：`host60-financial-restore-green.log` 银行 ECL 一项通过（0.01 秒），
`host60-restore.log` 保险 typed 损坏子账一项通过（0.01 秒）。实施者已亲读日志。
loan／scheduler／Simple clock／257 公司 collection 额外短 scope 尚待对应当前 integration 产物，不冒称这些全部已绿。
一次性指定 Luna medium 非作者审查已完成首次静态检查；报告更正 API 属开工前已存在的平行变更，不属于本次 restore 变更。
root 明确授权原同一 reviewer 收口当前两项绿色证据与最新 scale fixture 契约增量，不另启重复审查。
未自行运行 Cargo、Git 或生成绑定。
