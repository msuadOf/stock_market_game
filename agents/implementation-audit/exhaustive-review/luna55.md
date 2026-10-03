# 保险与工业复核记录 EOF 扫描

- 扫描对象：`agents/oop-refactor-implementation/domain/industrial-review.md`（67 行）、`insurance-result.md`（47 行）、`insurance-review.md`（62 行），共 176 行；逐行覆盖至 EOF。
- 产品版本：HEAD 为 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`；指定产品提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960` 是 HEAD 的父提交（HEAD 是 merge）。本记录对照当前 HEAD 的调用链与正式契约。
- 约束阅读：全文读 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`；另核对 `docs/company-accounting.md §2.6`、保险 production owner 与 caller。此为静态扫描；未运行测试、未改产品、未执行 Git 写操作。

## 全文覆盖矩阵

| 文件 / 行数 | 章节 | EOF 与原结论复核 |
|---|---|---|
| `industrial-review.md` / 67 | 1–9 元数据与审查范围 | 已读到：范围明确为 industrial 8 个文件及 ownership 测试；与此处保险日程候选不重叠。 |
| 同上 | 11–27 最终 SHA256 绑定 | 已逐项读完哈希表；仅绑定 industrial 源文件，不覆盖 operations/insurance。 |
| 同上 | 29–50 结论、领域语义、必要性、边界 | 原静态结论限定在所列改动 diff，未发现 industrial 迁移回归；不能据此推断保险跨层日程正确。 |
| 同上 | 52–61 有效发现 IR-01 | `BankBooks` caller 归属更正有 `operations/bank.rs` 与 `dispatch.rs` 接线证据；“已修正并复核关闭”只关闭该记录错误。 |
| 同上 | 63–67 非阻塞建议与局限 | 两项 industrial serde/多贷款覆盖建议仍是未来测试建议；末行明确未验整个 domain，与新增保险候选并不矛盾。 |
| `insurance-result.md` / 47 | 1–6 基线、需求与阅读范围 | 已覆盖；报告称保险写口限于 `insurance/**`，没有声明改过 `operations/insurance.rs`。 |
| 同上 | 8–17 owner / caller 动作矩阵 | 已覆盖；“advance_day ... 每日释放与发生/支付赔案”确认真实 caller，但没有记载赔案 schedule 在 coverage end 截止。 |
| 同上 | 19–24 领域语义与失败边界 | 已覆盖；赔案发生/支付分离是会计生命周期，不代表 coverage 期满后可发生新 claim。 |
| 同上 | 26–43 短测试与实施验证说明 | 已覆盖；列出的 behavior tests 覆盖 owner、快照、释放/重估、赔案支付及错误时序，没有 operations 日程跨过期末的 case。实施红绿测试未在实施侧执行的自述完整。 |
| 同上 | 45–47 最终门禁 | “本组没有待修有效发现”应限于 insurance owner 重构；其语境没有 operations 文件，不能拿来关闭本扫描发现。 |
| `insurance-review.md` / 62 | 1–8 元数据、基线、范围和依赖依据 | 已覆盖；审查范围明列 `insurance/**` diff 与测试，没有 `operations/insurance.rs`。 |
| 同上 | 10–24 最终版本绑定和 SHA256 | 已覆盖；绑定的是六个保险源码修改文件和一个 behavior test，不包含日常 caller。 |
| 同上 | 26–55 结论、会计语义、必要性、边界与错误路径 | 已覆盖；“未发现行为回归”针对上述完整保险 ownership diff。关于 `ClaimRegister` 由真实写口调用的结论成立，但只证明 owner 接线，不证明调用前的保障期守卫。 |
| 同上 | 57–62 验证范围与剩余限制 | 已覆盖；明确未审本批其他行业/整仓 diff，也未获取独立运行证据。因此不能以该审查作为 insurance operations 日程 EOF 验收。 |

## 当前调用链核对

- 契约依据：`docs/company-accounting.md:130-133` 将 insurance 的经营简化登记为“保险赔案按确定性保障日程发生、不经经营到期调度队列”。这描述保障日程内的新赔案发生；它不把既有未付 claim 的后续现金支付限制在 coverage period 内。
- 组边界：`packages/engine/src/company/insurance/premium.rs:62-116` 的 `establish_group` 接受 `start/end`、拒绝 `end <= start`，并以 `end.days_since(start)` 初始化责任单元数；ContractGroup 保留 `coverage_start` / `coverage_end`。所以每组有明确结束边界，日长为 `N` 的组截至 start + N。
- 唯一生产日程 caller：`packages/engine/src/company/operations/day.rs:271-281` 将 `FlowParams::Insurance` 静态分派到 `operations::insurance::advance_day`。`packages/engine/src/company/operations/insurance.rs:48-58` 对新组设置 `end = date + coverage_days` 并同日收保费；`:63-75` 遍历现存组，读取 `units_remaining` 与 `elapsed = date.days_since(coverage_start)`，只有服务释放由 `remaining > 0` 保护，而造 claim 的 guard 仅为 `elapsed > 0 && claim_every_days >= 1 && elapsed % claim_every_days == 0`，随后无条件 `record_claim`，再尝试付款。
- 可达反例：`coverage_days = 1, claim_every_days = 1` 时，新组在 start 当日释放唯一责任单元、remaining 归零；下一日 `elapsed = 1 = coverage_days`，代码仍造出并支付 `CLM-GRP-<seq>-1`。更长 coverage 也会在 `elapsed = N` 及每个后续匹配的间隔日继续造新 claims。`groups` 持续保留，没有 expiry 清理逻辑；重复日程仅在 `elapsed` 命中间隔时发 claim。
- 发生与支付要分别判断：`claims.rs` 中 `record_claim` 是产生已发生负债的动作，`pay_claim` 结算已发生金额。coverage 到期后继续支付此前发生、未付清的 claim 合乎该分离模型；本候选只指到期后由自动 schedule **新发生** claim。没有证据证明延迟报告的 coverage-window claim 应由 API 日期硬拒，因此不扩大指控到 `InsuranceBooks::record_claim` 的所有直接调用。
- 现有 day 模块测试中的 `insurance_params`（`operations/day.rs:434-444`）设 `coverage_days=1`、`claim_every_days=1`，但搜索到的相关测试只校验配置时长、持久化 pair mismatch 与调度恢复，并未驱动该 insurance 日程经过 start+1 核对 Claims。insurance `behavior_tests.rs` 也未接日常 operations caller。因此当前证据是源码级可达路径，尚无专项回归测试执行证据。

## 候选与反证

### EX-01：保障期结束后自动日程仍新建赔案

- 原文代码：`operations/insurance.rs:70-75`：`if remaining > 0 { release_service(...) }`，随后独立的 `if elapsed > 0 && ... { record_claim(...); pay_claim(...) }`。此处没有 `elapsed < coverage_days`、`date < coverage_end` 或对 `remaining` 的 claim-side guard。
- 旧结论复核：`insurance-result.md:17` 准确指出真实 caller 经日常 `advance_day`；`insurance-review.md:42` 准确证明写入经 `insert/apply_payment` owners。但这些均未检查 coverage-end gate。旧 reviewer 的“未发现行为回归”范围是 insurance owner diff，不包含未修改的 operations schedule，不能当成对此条件的反证。`industrial-review.md` 及 IR-01 是不同范围，既不支持也不推翻 EX-01。
- 契约对照：§2.6 用“保障日程”限定 deterministic claim event。自动 schedule 在 coverage_end 当日及以后仍造新 claim，与已建立的 coverage window 不一致。该条只作为游戏假设契约解释，不声称这是经官方核验的真实保险产品规则。
- 最强反证 / 限界：当前条款没有写出日程上下界的等式约定，且 `coverage_end` 可能采用排他端点；不过无论边界日 N 是否仍算保障日，elapsed>N 显然越过 end。仍可能有人主张简化模型把 `claim_every_days` 当作合同结束后的追索或尾部风险计划；但它被定义为“每隔 N 个保障日”且支付失败只保留 incurred claim，不支持无限延长新事故发生。故候选至少在 `elapsed > coverage_days` 上较强；`elapsed == coverage_days` 端点具体应纳入与否，宜按 start/end 半开约定确认。
- 必要性与范围：需由产品 owner 决定修复；本任务只留审计候选，不改产品源码。若确认要求止于 coverage_end，最小生产 guard 应基于 ContractGroupState 的已存 end（或统一明确 elapsed 与 end 的包含关系），而非仅用 remaining，因为到期后付款处理还可能必须继续。边界测试至少覆盖同日、最后一个保障日、end 日、end 后已到 claim schedule 的新 claim 不生成，以及到期前 claim 的后续 payment 仍可成功。
- 当前结论：有效高置信候选，产品规则语义未由此次审计裁定；现有 reviewer 结论需限缩到 owner 重构范围，专项行为待修或由 owner 明确否定该契约解释后再关闭。

## 扫描边界

此文 EOF 审计只复核三份历史记录与保险 schedule 的当前实现链；没有重跑父任务的编译/测试批次，没有重新取证 CAS 原文，也没有审阅整仓其他 domain 的完整 diff。产品变更应由实施者补测试后，再由未实施者复核对应完整 diff。
