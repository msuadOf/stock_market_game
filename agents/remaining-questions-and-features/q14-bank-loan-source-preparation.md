# Q14 Bank真实贷款来源准备

## 范围与既有语义

本批只让实际Bank Owner产出并保存贷款本金／每笔正额利息的明确来源事实，不接入CompanyOperations客户现金付款，不初始化客户现金、不选默认经营参数，也不修改当前LN到期处理。当前 `dispatch_maturity` 对Bank LN先收已提利息、再收本金；现贷款继续ACT/365F，逾期仍按原规则计息，本批不把来源独立改成另一清偿顺序。

本金来源直接来自 `issue_loan` 创建的实际 `BusinessKind::LoanIssued` 凭证；利息来源直接来自 `accrue_loan_interest` 创建的每笔实际 `BusinessKind::LoanInterestAccrued` 凭证，不从金额、journal位置或loan字典序补猜。`LoanAccrualItem.source` 正额为Some实际BusinessEventId，零额为None；零额继续推进余数及计息日期，但不生成凭证或贷款利息债务。

BankBooks保存必填 `loan_claim_sources`：只记录loan、真实source和本金／利息类别，金额与日期仍取唯一Journal，不保存第二份金额账。source只在真实过账成功后登记，失败不留下源记录；恢复须验证全部相关Journal的覆盖、source唯一、实际kind和正额科目行，以及明确存在的loan。贷款start/maturity原API已有显式输入，本批可原值保留，不增加默认期限。

## TDD与验证状态

红phase新增四项真实Bank fixture case：

- 两笔相同本金／利率贷款同日同额计提，要求返回彼此不同的真实source及持久关联，不按金额猜。
- 零额计提只推进既有状态，不产生source；同日重试无新凭证或来源事实。
- 将真实本金Journal source伪称利息，严格恢复必须拒绝。
- 本金与利息来源真实往返，缺失覆盖或重复source拒绝。

root统一构建的host44实际Engine二进制中，上述四case逐个精确匹配，全部真实失败；不是零case或只编译未执行。随后实施真实source捕获、原期限保存及严格恢复，host45统一构建后四项逐case实际各运行一项，四路并行全部通过。

Web严格Bank解析同步必填 `loan_claim_sources`、`start_date`、`maturity_date`，核对真实Journal种类／科目／日期、完整唯一覆盖及已登记借款人。只以明确source查Journal，不从金额或数组位置识别loan。新增四case首次运行三失败一通过，旧exact确实拒绝新增来源字段；实现后四项及相邻company-books九项、TaxOwner十八项合计31/31通过。独立复核进一步发现全局事件游标碰撞、Web Map会覆盖重复Journal source、Rust贷款未知字段校验漂移。Web补充空贷款也不绕过开局Journal的游标拒绝，复制Journal必须拒绝，合法高游标空洞允许；全局游标负例真实失败后实现守卫。TaxOwner三个合法fixture补正确的next_event_id，保留全部原断言。最终Web新增六项及相邻二十七项合计33/33通过，外部10000ms进程树deadline、Node case 10000ms、八路并发，整批约0.65秒。真实存档JSON由root统一重新生成，本批未手改。

Rust另外新增四项边界：全Journal游标碰撞（含空贷款）、未知贷款字段及重复Journal、原期限与逾期利息严格往返、发放资金不足／计息日期回拨／游标耗尽／实际凭证重复时完全不安装状态和来源。host46整批构建因无关Desktop测试调用缺失新参数失败，不能声称整批构建通过；其manifest明确产出的Engine二进制用于八项逐case测试，实际每项各运行一项，八路并发：原四项及逾期／失败原子两项通过，游标与未知字段两项真实失败（日志 `.tmp/checklist-wave4/q14-bank-source-host46/`）。随后补全Journal游标守卫及LoanState `deny_unknown_fields`；root host47统一fresh构建成功，当前Engine二进制八项精确case逐项实际运行一项，八路并发、整命令10000ms／每case9000ms进程树deadline，8/8通过，整批约0.12秒。日志与结果在 `.tmp/checklist-wave4/q14-bank-source-host47/`；不混同host45初四项绿色证据。

只做10秒以内短case及多核并行，不运行完整回归；非作者完整diff修复源码复核限定PASS，并亲读host46全部八项红绿及host47全部八项绿色日志签核；另独立复跑Web来源六case全部通过，见 `q14-bank-loan-source-review.md`。该PASS只覆盖真实来源准备，不能声称生产客户支付闭环或完整TaxOwner直接恢复完成。

## RE边界与后续接线

完整核读 `sign_presale(contract, project, buyer, units, price_total, date)`：没有due输入，date也尚未保存。不能为此凭空默认新的付款期限，本批先不动RE。来源标识与法律付款义务的关系、预售未付款转交付尾款不可双计，以及合同内息本／跨合同排序仍见 `q14-creditor-binding.md`。后续实际customer现金与Bank `Books` 的对应两侧过账必须在同一私有经营候选原子安装。
