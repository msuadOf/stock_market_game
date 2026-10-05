# Q14 客户债务债权人绑定

## 决定与范围

用户已经要求客户每笔收付有真实来源和去向、共享同一有限现金、按原合同到期及登记序偿付；债权人绑定落实这些既定边界，不新增经济参数或真实市场统计。普通经营偿付优先序不冒充破产清偿顺序，保留现有CAS 14/CAS 22依据与范围（`credit-delay-research.md`）。

独立基础账簿的债务登记必须显式传入 `DebtCreditor`，区分 `Company(CompanyId)` 与具名 `External`；不接受 `Customer(CounterpartyId)` 作为债权人身份。公司、客户即使标识文字相同也仍是不同命名空间。`CashEndpoint::Company` 标识模拟内公司端点，但基础模块不持有该公司的 `Books`，不能把输出流水宣称为公司现金已经过账。

目标接口中，`repay_due` 与 `recover_written_off` 不再接收调用方指定的收款人，分别依各笔债务自己的 `creditor` 生成现金流。不同公司债务共享一个客户余额，按原到期日与登记序全局分配，而不是依调用公司顺序付款；末笔允许部分付款，残额及原逾期起点不变。核销不免法律债务，核销后回收仍付原债权人，不重复清偿普通应收。旧API及缺债权人字段的旧账簿不兼容、不补默认值。

## TDD与接线边界

红phase先引入必填债权人与强类型端点，暂保留旧算法使用调用者destination。root统一host37批次整体因Server `api_contract` 的 `E0382` 编译失败，不能声明全host通过；其中新的Engine libtest实际生成成功，来自 `.tmp/checklist-wave4/host37-partial-binaries.json`。`engine-fd39ddbb13be65af --list` 实际确认20项客户账簿case，整批10000ms、每case9000ms独立进程树监督，8进程并行、Rayon 4运行，原16项及必填／命名空间guard共17项通过，新增3项均实际业务断言失败（底层101，整批xargs123），日志在 `.tmp/checklist-wave4/q14-creditor-host37/`。真实红分别证实各债权人款项被转给调用方同一公司、核销后回收转给错误公司、同时篡改事件与流水收款人仍通过旧恢复校验；不是fixture错误或零case。

绿phase已经移除旧destination入参并只使用债务绑定端点；原收款人、金额、到期日及幂等断言不弱化。旧付款事件冲突测试因收款参数删除，改为同事件不同日期，仍要求明确 `EventPayloadConflict` 且不再付款。恢复核查付款回执、核销回收流水与该债务债权人一致，并拒绝给付款／回收命令添加destination字段；另加独立核销回收流水及命令篡改guard，共21项case。root统一host38构建55.59秒成功（`.tmp/checklist-wave4/host38-binaries.json`），实际当前 `engine-fd39ddbb13be65af --list`确认21项，全命令10000ms、每case9000ms进程树监督，8进程并行、Rayon 4，21/21通过，每case实际1项、0失败、0.00秒，整批约0.36秒，日志 `.tmp/checklist-wave4/q14-creditor-host38/`。非作者完整source/diff和21项测试源审限定PASS，最终绿色日志与记录再由其亲读签核，见 `q14-creditor-binding-review.md`。

本批不接入生产 `CompanyOperations`、工业/银行/地产/保险收款调度或宿主存档，也不设置客户开局现金与经营收支模板；完整生产依赖仍见 `q14-finite-ledger-followup.md`。真实公司端点与客户余额的另一侧 `Books` 过账须在后续同一个私有经营候选中原子安装，不能由两边各自生成一份现金。生产应收映射必须联合公司身份和原开项身份形成无歧义、稳定且唯一的债务身份，不能只取不同公司可能重复的 `AR-<event>`，也不能假定标识内不会出现某个分隔符而简单拼接。

## 联合应收来源身份

当前生产取值域核读后，联合身份不能只支持一个 `OpenItemId`。工商 `CreditSaleOutcome.receivable` 与地产 `DeliveryOutcome.receivable` 都按各自账套 `BusinessEventId` 生成 `AR-<event>`；四种行业账套各有 `next_event_id`，开局都从2推进，Journal只检查本账套来源唯一，不是全市场来源号。经营流 `next_flow_seq` 也逐公司从1开始：银行生成 `LN-<seq>`，地产生成 `PS-<seq>`，保险生成 `GRP-<seq>`。不能删公司身份或以某个数字source全市场唯一为由省略它。

联合身份批使用 `CompanyClaimIdentity { company, source }` 的具名构造明确来源类别，`CompanyClaimSource` 区分工商应收、贷款本金、每笔贷款利息、地产预售合同、地产交付尾款、保险应收保费。银行同一 `ContractId` 的本金与利息不是一笔义务，每次正额利息计提须带实际 `BusinessEventId`；同笔付款、技术重试或核销后回收不重新分配另一债务身份。保险赔案是公司应付，不能放进客户欠公司的来源类别；银行存款也不是客户对银行的债务。

具名编码采用 `DebtId::for_company_claim`，在校验公司与各typed来源身份后，按独立职责前缀、固定来源类别及UTF-8字节长度编码公司和所需来源段。长度使用Rust字符串字节长度，不以字符数或JS长度混用；分隔符、数字、中文及其他Unicode均保持原文；利息源数字使用canonical u64十进制字符串，保留全域精度。相同输入产生相同id，不同公司、来源类别或利息来源不得碰撞，不将债务身份当成金额、委托序号或证券账户。

联合身份红phase先添加强类型身份与七项短case：不同公司相同`AR-2`及分隔符组合不碰撞、Unicode/原文/编码稳定往返、六来源及同loan不同accrual独立、两个发行人同开项的共享客户150分实际优先分配100/50及重复登记拒绝/恢复幂等、强类型身份严格字段与全u64利息来源字符串恢复，以及generic登记／严格账簿恢复对编码来源公司与债权人错配的各自拒绝。root统一host39构建56.43秒成功，实际Engine `--list`确认28项；整批10000ms／每case9000ms监督、8进程并行、Rayon 4，22项通过、6项真实红，case各实际1项。四红来自明确未实施的编码stub，另两红是真正的错误来源公司被generic登记／serde恢复接受，不混同为stub失败。日志保留在 `.tmp/checklist-wave4/q14-claim-host39/`。

随后已移除stub，实现来源类别及UTF-8长度编码、严格反解、原文往返与登记／恢复债权人交叉校验，并增完整非法编码guard（非canonical长度／溢出／截断／UTF-8半字符／未知来源／多余数据）及最大u64利息来源的ID往返。host40统一构建65秒成功，实际 `engine-48220fcc6e1a5073 --list`确认29项，采用相同deadline及并发参数，29/29通过、case各实际1项0失败0.00秒，整批约0.51秒，日志 `.tmp/checklist-wave4/q14-claim-host40/`。

非作者源审提出补充严格Claim顶层及source变体字段的负例。既有case后加8个缺字段／未知字段／重复字段断言，先解析为合法JSON再要求强类型拒绝；因它们在host40编译期间写入，没有用同名case或二进制常量猜测新断言已经执行。来源编码实现与API保持冻结，再由root统一host41 fresh构建58.53秒，实际 `engine-48220fcc6e1a5073` 的当前29项全部编入并执行，包括新8字段断言；相同deadline、8进程并行与Rayon 4参数，29/29通过，每case实际1项0失败0.00秒，整批约0.48秒，日志 `.tmp/checklist-wave4/q14-claim-host41/`。非作者完整及增量源码审查限定PASS，最终绿色证据亲读签核见 `q14-company-claim-identity-review.md`。

显式自定义 `DebtId` 与构造结果重复时仍明确拒绝，不覆盖或悄悄换id；非公司编码的具名债务明确保持自己的身份，不猜成某个公司来源或兼容旧编码。身份构造不创建客户、销售、应收、现金或自动选择默认经济参数。联合身份实现只使用HEAD已有的 `CompanyId`、`OpenItemId`、`ContractId`、`BusinessEventId` 及既有有限账簿；当前代码没有引用待提交的 `canonical_u64_decimal`。本地 `business_event_decimal` 只为本新类型的 `BusinessEventId` 字段提供严格字符串Serde，复用同一 `parse_accrual` 做JSON与身份编码解析，不接受数字兼容输入，也不改变现有Journal全局契约；其他AccountWire依赖不能仅因同步统一编译而自动算入本批源码依赖。

生产Owner adapter接线需要保留实际结果身份，而不是从金额猜测：工商由赊销返回开项映射，地产由签约和交付返回身份分别映射，保险由建组返回的实际group映射；银行本金取真实发放合同，利息取每次真实正额计提的source。当前 `LoanAccrualItem` 只含loan／days／amount／余数，没有source；Journal分录也没有loan身份，因此多个同额贷款同日计提不能靠金额、行位置或组顺序补猜。后续需在Owner真实处理器产出明确source关联，零额计提只推进计息状态，不能登记零债务；重试或日终恢复不得重抽source。各收款adapter再在同一经营候选里按源类别分派到 `collect`、`collect_loan_principal`、`collect_loan_interest`、`collect_presale`、`collect_final` 和 `collect_premium`，只使用客户实际付款额，不重复确认收入、保费负债或本金。

地产还缺合同开立／付款到期事实：当前 `sign_presale` 的date参数未保存，不能从后续collection日期补猜原due；已登记全额预售价为客户债务时，交付后的同一未付部分转入FinalAR不能再登记一份重复债务，须保持同一法律义务或显式原子重分类关联。六类身份只区分来源，不授权自动设定付款期限或同时创造两份债务。当前未修改这些生产Owner与dispatch。

银行现行LN到期Owner先收已提利息、再收本金；若仅按本金发放先登记、每笔利息后登记，把共同到期的各分量直接塞入通用登记序可能反转这个合同内顺序。来源身份独立不授权改变现合同内处理规则，生产adapter必须明确合同内分量与跨合同到期／登记排序的协调；不能伪造更早登记、修改原due或凭id字典序偷偷达成某个排序。本批不选择新的经济或清偿参数，也不声称完成这些接线。
