# Q14 客户债务债权人绑定

## 决定与范围

用户已经要求客户每笔收付有真实来源和去向、共享同一有限现金、按原合同到期及登记序偿付；债权人绑定落实这些既定边界，不新增经济参数或真实市场统计。普通经营偿付优先序不冒充破产清偿顺序，保留现有CAS 14/CAS 22依据与范围（`credit-delay-research.md`）。

独立基础账簿的债务登记必须显式传入 `DebtCreditor`，区分 `Company(CompanyId)` 与具名 `External`；不接受 `Customer(CounterpartyId)` 作为债权人身份。公司、客户即使标识文字相同也仍是不同命名空间。`CashEndpoint::Company` 标识模拟内公司端点，但基础模块不持有该公司的 `Books`，不能把输出流水宣称为公司现金已经过账。

目标接口中，`repay_due` 与 `recover_written_off` 不再接收调用方指定的收款人，分别依各笔债务自己的 `creditor` 生成现金流。不同公司债务共享一个客户余额，按原到期日与登记序全局分配，而不是依调用公司顺序付款；末笔允许部分付款，残额及原逾期起点不变。核销不免法律债务，核销后回收仍付原债权人，不重复清偿普通应收。旧API及缺债权人字段的旧账簿不兼容、不补默认值。

## TDD与接线边界

红phase先引入必填债权人与强类型端点，暂保留旧算法使用调用者destination。root统一host37批次整体因Server `api_contract` 的 `E0382` 编译失败，不能声明全host通过；其中新的Engine libtest实际生成成功，来自 `.tmp/checklist-wave4/host37-partial-binaries.json`。`engine-fd39ddbb13be65af --list` 实际确认20项客户账簿case，整批10000ms、每case9000ms独立进程树监督，8进程并行、Rayon 4运行，原16项及必填／命名空间guard共17项通过，新增3项均实际业务断言失败（底层101，整批xargs123），日志在 `.tmp/checklist-wave4/q14-creditor-host37/`。真实红分别证实各债权人款项被转给调用方同一公司、核销后回收转给错误公司、同时篡改事件与流水收款人仍通过旧恢复校验；不是fixture错误或零case。

绿phase已经移除旧destination入参并只使用债务绑定端点；原收款人、金额、到期日及幂等断言不弱化。旧付款事件冲突测试因收款参数删除，改为同事件不同日期，仍要求明确 `EventPayloadConflict` 且不再付款。恢复核查付款回执、核销回收流水与该债务债权人一致，并拒绝给付款／回收命令添加destination字段；另加独立核销回收流水及命令篡改guard，共21项case。root统一host38构建55.59秒成功（`.tmp/checklist-wave4/host38-binaries.json`），实际当前 `engine-fd39ddbb13be65af --list`确认21项，全命令10000ms、每case9000ms进程树监督，8进程并行、Rayon 4，21/21通过，每case实际1项、0失败、0.00秒，整批约0.36秒，日志 `.tmp/checklist-wave4/q14-creditor-host38/`。非作者完整source/diff和21项测试源审限定PASS，最终绿色日志与记录再由其亲读签核，见 `q14-creditor-binding-review.md`。

本批不接入生产 `CompanyOperations`、工业/银行/地产/保险收款调度或宿主存档，也不设置客户开局现金与经营收支模板；完整生产依赖仍见 `q14-finite-ledger-followup.md`。真实公司端点与客户余额的另一侧 `Books` 过账须在后续同一个私有经营候选中原子安装，不能由两边各自生成一份现金。生产应收映射必须联合公司身份和原开项身份形成无歧义、稳定且唯一的债务身份，不能只取不同公司可能重复的 `AR-<event>`，也不能假定标识内不会出现某个分隔符而简单拼接。

## 下一批联合应收身份计划

计划提供具名构造 `DebtId::for_company_receivable(company, receivable)`，在校验两个非空身份后，按独立职责前缀和UTF-8字节长度编码两段原字符串：`company-receivable:<公司字节数>:<公司原文><开项字节数>:<开项原文>`。长度使用Rust字符串字节长度，不以字符数或JS长度混用；分隔符、数字、中文及其他Unicode均保持原文。相同输入产生相同id，不同公司或开项不得碰撞，不将债务身份当成金额、序号或证券账户。此处是待TDD实施方案，不是当前已有API。

短测先覆盖：不同公司相同`AR-1`、公司／开项双方含冒号与数字造成的简单拼接歧义、中文与Unicode、空白身份拒绝、相同输入稳定；再用真实独立公司债权人在共享客户簿登记、付款和严格恢复，证明两笔债务不会被覆盖或混付。显式自定义 `DebtId` 与构造结果重复时仍明确拒绝，不覆盖或悄悄换id。生产接线还须将构造时公司与实际 `DebtCreditor::Company` 交叉验证；身份构造本身不创建客户、销售、应收、现金或自动选择默认经济参数。
