# R4 ordinary 税日生产接线状态

日期：2026-10-29。唯一 writer 为本 worktree；基线 `cface7630b1c04c546ea1f2ffd87219977caacec`，未修改根 dirty、Host、Calendar 或 main。未运行 Cargo，也未进行大范围 hash；正式执行由 root 共用唯一资源槽安排。

## 可采红与 fixture 修正

原始联合执行收件位于根仓库 `.worktree/company-write-host-2026-10-28/agents/branch-completion/company-write-host-2026-10-28/combined-red-01/joint-native-and-export-02/`；`cases.json` 的原选集、exact 名称、panic marker 未改。

- `r4_tax_01`、`r4_tax_02` 已实际精确在 `ordinary REG 必须由真实 civil 事务推进，不能仍停在开局前日` 失败，child 101，stdout marker、stderr 空；作为普通生产 frontier 的 TDD 红。
- 原 `r4_tax_03` 在 `declare_dividend` 提前报 `Finance(DividendUnsupported("尚无完整自然年度税后利润账簿事实，不能核定法定公积金"))`，不是目标 REG 红，保留原失败原件。
- 净测试提交 `1133d042` 仅将 helper 参数化，01/02 保留 2 个月前史，仅03使用 17 个月。Monthly 从 2030-06-01 回退到 2029-01-01，实际生成 2029 全年与 2030-01 至05 的利润及税务账簿；没有手填派生事实、注资或削弱公积金门。17 是该起点最短完整年度前提。
- 未实施 `gpt-6.1-sol` / `xhigh`、task `tax_fixture03_review_29` 已完整审单test差量三问，无阻断项。它实际重读原03失败、`state::create` 与 `latest_complete_fiscal_year`；没有运行 Cargo，此结论不能当新 fixture 已红或已绿。
- 修正03须由下一必要联合有界图在 fixture-only 来源取真实目标红，之后才能实施 capture 特有生产路径；不得将未来实现源码回填为历史红来源。

## 已写但待审、待运行的 01/02 生产批

独叶 `session/ordinary_tax_day_end.rs` 在真实 `civil_information` shadow 的经营与披露候选成功之后、INFO prepare 之前执行。只消费已安装账户、最终个人确认、冻结日历与原 live World 真实末拍探测结果；每证券构造唯一 typed `ShareSettlementDay`，调用 `TaxDayBoundaryFacts::prepare`，再以真实账户持仓作 `reconcile_registry`。同 shadow 的 Companies 由既有 adopted move 与 `PreparedCivilFields` 固定成功尾随同一 Admin pending 安装。

本片没有组端现金或股份操作，没有付款协议、税收缴、`Paid` 或 `Credited` 安装；同日 capture 尚未实施，遇 due 未登记分红会显式拒绝。首次净买入入册个人使用现有维护语义开空税账；既有持有人不得事后补历史税账。历史公司/unit movement、旧 untyped REG、未核限售及个人 `BonusShares` 门不放宽。当前仅为 native runtime civil 接线，不宣称 wasm 串行面已接通。

生产批已交未实施 `gpt-6.1-sol` / `xhigh`、task `tax_ordinary_production_review_29` 审完整 diff；尚无独审结论与正式 green，不能宣称生产完成。

## 规则依据与适用范围

沿用既存官方原件：根仓库 `agents/branch-completion/tax-basis-sources/85-correct-fgk.txt` 和 `101-correct-fgk.txt`。本轮实际重读85号第三条 FIFO／每日日终净增减，101号第三条衔接85号其他操作及第五条2015-09-08施行。本批不调整税率、扣缴时点或证券类别规则。正式既存边界见 `docs/trading-rules.md` 个人税候选节；来源批交接见 `agents/branch-completion/tax-admin-projection/tax-source-2026-10-27/production-handoff-2026-10-28.md`。

fixture 法定年度依据沿用公司法2023修订第210条（2024-07-01施行，2026-10-05取证）和企业所得税法第53条（公历年度；2018修正的官方全文；2008-01-01施行），分别登记在 `agents/remaining-questions-and-features/dividend-research.md` 与 `q23-tax-official-research.md`。不把 Simple 账簿游戏简化扩称完整税法或 CAS 实现。

## 恢复后的未实施复核（2026-10-29）

本次实际重新派发 `gpt-6.1-sol` / `xhigh`，task `tax01_02_production_review29`，只读核完整四文件生产 diff 与 `ordinary-production-01.patch` / fingerprints，无 Cargo。结论为暂不通过：

1. `AShareIndividual` 已有个人持有人缺税账时，当前 restore 仍接受；ordinary caller 只为首次净买入开空账，会让缺失个人的税日静默缺席。应显式拒绝，不能补历史。
2. 已有匹配证券但归属不存在账户的空税账当前 restore 仍接受；boundary 的空物理 lot 与空税 lot 一致，caller 会为孤儿税账安装零税日。应显式拒绝，不能补账户。

Tax04 tests-only 净提交 `87712534` 已获该未实施 Sol 源审：删除真实有 opening lots 的个人一本税账，走真实 closed `end_civil_day`，预期在 `expect_err` 失败，marker 为 `已有个人持有人缺失税账必须拒绝，不能静默跳过税日`。这是未运行测试，不能算实际红。Tax05 对称孤儿边界另交 fresh 窄源审；root 将本批新增边界冻结到04/05，不扩财务领域。当前 integration 23 exact 已冻结，04/05进入下一必要联合图；生产覆盖守卫必须在真实目标首红之后实施。

Treasury 负净额仍须保持旧显拒能力门。新 ordinary caller 尚未单列拒绝；未实施 reviewer 只证明公开 runtime ExternalPlan 来源通路可以输送专户委托，默认玩家路径拒非 Player且专户无自主策略，尚未证明默认生产可达 Treasury Sell。不能把此边界记成已证默认用户漏洞，也不能把“真实最终确认”当作合法卖出资格。守卫及最窄真实来源验证待后继门，未宣称通过。

本片继续只负责 ordinary REG、税日与同日 cash-dividend capture 来源，中央 mixed tax-stage/真实付款及 collection 由中央 owner负责同 pending/id 后续接线。没有付款真实回执时不会安装 `Paid` 或 `Credited`。


Tax05 tests-only 净提交 `fabf1895`（在 `87712534` 之后），只追加44行 `r4_tax_05_orphan_tax_book_rejects_without_creating_account_or_tax_day`。fresh `gpt-6-luna` / `high`、task `tax05_fixture_review29` 窄审 fixture，报告后结束；其源码线索已由本 writer 实际重读 `session/persistence.rs::validate_company_domain`、`session.rs` 资本 restore 后复制 actions、`TaxDayBoundaryFacts::prepare` 对空 lot 的处理逐项复核。预期 marker 为 `孤儿个人税账必须拒绝，不能为不存在账户记录税日`；仍无实际红。该窄审没有另查官方原件，其领域依据沿本批已由 Sol 核过的 ADR-0040 与85/101号来源，不把 Luna 自报当作独立终审。

当前已提交只含测试；四生产文件 WIP 与 frozen `ordinary-production-01` 原样一致。root未开放本 writer Cargo 窗口，唯一联合构建归 integration。没有运行测试、没有手工回填执行证据、没有push。


## Tax03 实际目标首红与 capture 实施

真实目标首红现已亲核：integration `joint-red-r2-2026-10-29/feature-company_r4_tax_production-r4_tax_03_registration_day_captures_two_plans_and_keeps_real_tax_lots_on_restore` 原 stdout 在 test line129 精确失败 REG marker，child101、wall0.709s、stderr0、case-result expectedMatched=true。17月 fixture 已实际通过年度利润与 strict restore 前提。二进制 SHA `b3c8b421cf6aee9a67b0ee8d7c66ed6011a1226b75f62a49d8a2da892395e214`。整批因别的 collector 诊断合同失败不能报全批通过，此实际 Tax03 红原件保留。

在此之后才移除 capture 未接门，按 capture-plan 编写 typed post→真实 snapshot→批准 gross 勾稽→private attribution→register_dividend。多方案的operation锚核不可变 prepared.books 市场尾，不对逐方案递增的mutable seq。仍不写付款或Paid。

完整四产品文件冻结为 `capture-production-02.patch` 和 `capture-production-02-fingerprints.json`；交 fresh 未参与实现 `gpt-6.1-sol` / `xhigh` task `tax03_capture_review29` 只读三问审查。尚未运行生产绿测。04/05守卫尚未实施，等待后继Host必要首红；这两已知独审发现不能被03 capture掩盖。


## 后继 ordered-stage tests-only

root明确授权首三门范围后，已新增最窄内部placeholder + 3个真实World case，保存净提交 `0974c008`（4paths、230新增行）；该净commit不含549产品祖链。入口统一显拒 `R4 ordered stages 的同 pending 实际来源尚未接通`，没有stage生产实现。冻结文件为 `stage-three-red-source-01.patch` / fingerprints，patch SHA `c0bdedd275cb2644ef5fcaf6c855fa51f168cfda75d210fc562f7f5d0a54819f`。

未参与实现 `gpt-6.1-sol` / `high`、task `stage_three_red_source_review29` 只读完整四文件三问，裁决 `APPROVED_TESTS_ONLY_ADMISSION`。fixture路径100→140→110→100、真实PaymentDebit不足错误、原始inspect原因与未来flat索引断言均通过源审；尚未Cargo/实际红。当前仅证明测试设计覆盖完整GroupAccount与同World失败后健康续行，不能声称已测计划/个人/library/facts/水位全域回滚；真实生产仍须补同working来源与借原typedoperand的只读join，不以索引/owned投影冒证明。

exact与marker保存在 `stage-three-red-cases.json`；配置可用下一必要feature engine --lib构建，不插入当前已冻结Host/Calendar/主图序列。当前实际HEAD `0974c008`，产品源及测试已保存，只有本主题agents记录未跟踪。无Cargo、无push。


## 当前交付口径

- ordinary/capture 产品净片为 `549ab3f9`（仅5paths：四产品与 `docs/trading-rules.md`），等 `capture-production-04.patch`。未参与实现 `gpt-6.1-sol` / `xhigh` task `tax03_capture_review29` 已完整复核5文件/patch指纹，限定源码通过；其两项有效03发现已修：统一 final `validate_cash_dividend_books` 核批准日期/金额/法律来源/付款对应；今日 due 未capture（包含无registry但有分红）在安装前显拒。仍未实际生产green、未运行两个拒绝反例；04/05/treasury不在限定通过范围内。
- integration owner已收到唯一5path净片并负责01/02/03下一必要green，不能整合本树父链而带入04/05或stage stub。
- Host owner已只接Tax04/05的73行tests-only（`87712534`、`fabf1895`）进后继必要26图；04/05尚无实际红，本 writer没有提前写其守卫。Tax04/05原expected marker及独审说明见前文，不能把source审当实际首红。
- 中央后继只有 `0974c008` 净4path tests-only/stub，尚无实际首红；root已确认排到当前冻结图之后。没有stage producer、真实资金付款/collection或新税额算法。
- Treasury负净额显拒来源验证仍有证据缺口；旧候选原门未删除，ordinary新producer默认Session卖腿不可达未证，不能宣称已全面覆盖。

当前本树只留本主题 `agents/` 工作记录未跟踪；产品和tests已保存本地，无root dirty改动、无push、无Cargo。后继动作必须以真实04/05、stage3首红或01/02/03生产绿原件为触发，不回填旧红来源。


## 2026-10-29 Tax04/05 实际红、守卫与编译接缝修正

Host 实际批 `native-middle-02`，session67562 / WAIT0 / chunka46e04，共179.116s（外179395ms），21green+5精准red；front SHA `f77ff6374ecd5c31a39cdb2cceb0f81f2ccacf6f7630f5bdd995c747550fcae9` 已亲核。两税原件是同目录 `tax-company_r4_tax_production-r4_tax_04...` / `05...`：stdout最早在373/416的expect_err收到真实Ok CivilDayEndReport，child均101、约0.070s、stderr均0、expectedMatched=true。没有把提前restore失败当目标红。原Host Sol实际全批报告 `actual-review.md` 已明确两税可采，非最终PASS；完整SaveSlot不变尾断言尚待green。

在此实际首红之后才编写守卫：净commit `6559a938`，仅 `session/ordinary_tax_day_end.rs` +30。全税账归户在earlyreturn前核真实账户存在（含Exempt显式账）；AShareIndividual的既有Personal holder必须已有对应税账，首次正净买入维护保留，不补历史或账户。fresh未参与实现 `gpt-6.1-sol` / `high` task `tax04_05_guard_review29` 亲核真实红+完整30行diff，裁决 `APPROVED_SOURCE_ADMISSION`。冻结 `tax04-05-guard-01.patch` / fingerprints，净源SHA `c3d26b8dd86f267829e6eb765b6393f52d5a04df349df909bab8fe371c0fabf1`。该片已交integration后继04/05green，不插当前36。

integration36真实session65477 / WAIT1 / chunk5901a8，41.419s、0case，原 `feature-build.stderr:4959-4963` 报 E0616：原549的普通capture直接读CashDividendTaxBook私有operation_seq字段。旧静态审未发现该编译错误，不能称36通过。实际亲核该book没有现成getter/save API，未编造可用方法，也不serde绕边界。

最小编译修为净commit `e78cb9e8`（仅2paths，5增1改）：CashDividendTaxBook新增 `pub(crate) const fn operation_seq(&self)->u64` 原样只读返回字段，ordinary保留原比较只改调用getter。字段仍private、serde未变，不可变prepared.books市场尾及多方案来源锚不变。独立fresh `gpt-6.1-sol` / `high` task `tax_operation_accessor_review29` 亲读原E0616与完整两处diff，限定源码通过；冻结 `tax-operation-accessor-01.patch` SHA `ed156a0c4a7ef71dcc883560a5bc30d5f33076094a91a663868f9f02136be979`。已立即交integration当前36必要续图，仅取此净片不能带父guard/stage。

当前HEAD `e78cb9e8`，本writer仍未Cargo/未运行green，只有agents记录未跟踪。中央stage3 `0974c008`仍未实际首红，生产stub原样，绝不提前实现。守卫和编译fix源码独审不代表运行通过；Treasury负净额、真实资金付款/collection、完整raw回滚和双INFO点仍保留后继门。

## 2026-10-29 joint43 实际证据与 ordered-stage 生产准入

integration 的 `joint-43-2026-10-29` 为真实 session93984 / WAIT0 / chunk3703b6，wall144.038s；43项中38green与5项预期red。`actual-wait-cleanup-front.json` SHA 为 `fef15b4bc3902e57872f9bf4fe99582f8066b83f84a419ebaebd1b9927dfd20a`，已实际读回核对；Tax01–05 的各自 case-result 均为 child0、唯一1pass0fail、exactPassed和expectedMatched=true。因此 ordinary REG、capture 与缺账／孤儿守卫这五门现已有实际green，不能将整43报成全绿。

stage01/02/03 均在 stub 返回 `R4 ordered stages 的同 pending 实际来源尚未接通` 后命中原目标断言；三门 child101、0pass1fail、约0.02s。`--nocapture` 下标记位于 stderr，stdout为测试汇总。01在line106期待成功，02在line135核真实PaymentDebit原因，03在line163核独特inspect原因；原stderr、case-result与status保留，不回写历史来源。

只有实际首三门red之后，才实施真实stage producer。净commit `848eefec`，八路径778增37删；冻结 `stage-production-01.patch` SHA `c52b3e76fb819eec20c1975dc0dc3950e99cff3066ae691fb87236764b9f481c`。一份原flat typed操作权威，layout只记录stage计数／tax stage；Group复用唯一checked-apply循环，真实税stage before/after来自同working，不从最终净额反推；World仍复用唯一Admin pending/id、最终INFO、inspect与Commit/Abort。inspect借用World保留的原typed operand；无双INFO能力声明。

未参与实现的Sol task `sol_stage_producer_review29` 已实际审完整八文件与指纹，最终裁决 `APPROVED_SOURCE_ADMISSION`。其有效发现已修复：损坏税stage数值来源现在于协议回执验证调用唯一tax projection core，沿terminal失败路径处理，不能误作健康inspect业务拒绝。报告为 `stage-production-01-independent-review.md`。这只是源码准入，真实编译及stage01–05green由integration安排，不能提前报通过。

stage04验证原operand地址借用与六类实际通道损坏；stage05补三组、两股、非零owner、tail padding、真实Calendar、完整账户／PlanBook／完整个人态及World source失败保留和同payload续行。04/05为实现之后的回归保护，不能登记成历史首red；05也没有逐一直接snapshot所有私有library/facts/watermark/signature/dirty字段，不扩称全私有raw回滚已验。

接收方integration保留其较新 `protocol.rs` WASM `HostAborted` 与 `group.rs` Group consumer/tail内容，因此只六份整文件hash与此freeze相同；其两处接缝由接收方Sol独审，不能用本树旧整文件覆盖。当前本树HEAD `848eefec`，产品源不再变动。

## 后继真实付款与collection tests-first

已按root授权交fresh Luna在隔离leaf树只新增 `session/ordinary_tax_day_end/payment_runtime_tests.rs`，预备AShare gross到账一次、Flat10%净额与真实代扣、真实卖出后原登记股利收缴三门。此刻未注册、未Cargo、未取得实际首red，付款生产没有提前实施。

现有 `distributions_day_end_candidate` 只形成gross credit、withholding、payment与collection候选；其中 `available_cash` 必须为实际GroupAccount.raw cash而非含buy purse的Session总现金。后继接线须在首Prepare前的同源完整baseline上生成候选与最终INFO公司facts，借同次原typed operand核实际stage变化，并只在同一Commit成功尾安装Companies／Paid／collection与ack；不得将候选字段或最终净额当真实资金回执。

## stage 实际50构建失败的窄修正

integration owner报告50真实session35557 / WAIT1 / chunk3ea2a4，wall59.460s，compile FAIL且0case；唯一E0282指向 `stage_information_tests.rs:116` 只含panic的inspect闭包，泛型P不能推断。该实际失败不能计为stagegreen，原FAIL保留；本writer未运行Cargo，也未自行读取完整runner raw。root授权integration owner唯一writer就地修正并安排同50必要续跑。

本树同步最小1行类型注解 `|_, _| -> Result<(), RuntimeError>`，原panic及后续强断言均不变，产品零改动；修后文件SHA `f4163ef996a53732bbafa1e82f166f16171f865d60923fa5da8da6895ee5fd1e`，`git diff --check`通过。已交原未实施Sol reviewer窄复核并在其原报告追加编译边界；旧freeze原件不改写。该静态修正不等于下一次真实编译已通过。

窄注解经原Sol复核后存本地净commit `4ad5a291`；只含1行test类型注解。

## stage 实际50r2绿与付款三门源码冻结

integration `joint-green50-r2-2026-10-29` 为实际session89211 / WAIT0 / chunk1aeb8d，wall113.314232214s、外113.552s。已亲核front（SHA `ea0b1e66839e7a26ef80cc4a1cb100dad6c7db9648cfe12532b31306ede5a02b`）、stage01–05原stdout/stderr、child-status及case-result；五门均child0、唯一1pass0fail、exactPassed/expectedMatched=true，分别约0.015/0.019/0.015/0.043/0.021s。05的stderr是原已有INFO检查PROBE，没有panic；其余四份stderr0。未hash大artifact或重跑case。

接收方未实施Sol的 `actual-review.md` 裁决 `APPROVED_ENGINE50_R2_ACTUAL_ALL_GREEN`，其逐项亲核全部50门及source绑定、j8/8case池/10秒子树与300秒共享预算和清场。该实际票范围包括Tax01–05、tax core5、stage5及旧flat1，但不是Session付款／collection／Paid或整树验收。此前50编译FAIL/0case仍原样保留，不能反写。

付款3门Luna leaf已交付最终原稿SHA `1e8ee73762adde2003c31011f48aec3b1a7858676701920ce57a03fbd45fb872`，未Cargo。writer亲核真实接口后在本树加入最短17月全年、公共strict restore前件、实际Calendar的ex/pay日推导、restore后测试资源拓扑3组与真实Owner/tail断言、postmarket现金基线、02首先核现金marker、03唯一真实Trade与最终个人确认及买方无已登记股利。没有改变普通生产实现或付款候选。

当前tests-only净冻结为两路径：`ordinary_tax_day_end.rs`只增加3行native test module注册（SHA `ecad092789c498c068db42d4beee126b27a54704ec5571d22801c514aa6b45a1`），新 `ordinary_tax_day_end/payment_runtime_tests.rs` SHA `81a21a6d8a9fbf2dbc6e062fa76f27010c5a60c578a9142f9e4f610deae34bca`。`payment-three-red-source-01.patch` SHA `e584d4c089767b5bf0695e67a77637b62f02d92953ce8036b6334802286a0385`；exact/marker见 `payment-three-red-cases.json`。Rustfmt单文件解析与空白检查通过；未编译、未实际red。已交未参与实现Sol task `stage_three_red_source_review29`完整三问独审，待准入后给root集中排窗；不插已冻结50r2来源。

独立Sol最终 `APPROVED_TESTS_ONLY_ADMISSION`、无有效源码发现，全文 `payment-three-red-source-01-independent-review.md` 已实际读取；其亲核两个官方原件、ADR0040及全部真实接口/完整diff。净commit `779a7f91`只含此两路径474新增行，已经向root与integration交 `READY_FOR_ACTUAL_RED`。当前仅本主题agents未跟踪，付款生产未实施、未Cargo；下一动作等待root集中必要3exact实际red，不能由stagegreen或静态审替代。未来付款失败回滚、部分收缴/rawcash不足及INFO资金接缝仍是独立后继门，三正向测试不冒充这些边界。

付款3精准red runner位于本主题 `payment-red-runner-01/`，复用实际50r2骨架，五份监督核心byte不变，只裁名单／计数／owner／绝对work路径及red判据。fresh Luna leaf初稿后由writer接手必要机械修正；原Sol窄审 `APPROVED_MECHANICAL_CONTROL_DELTA`。integration唯一writer已经精确接两test源与经审Host actor最小修，明确授权本writer在自己agents一次现场freeze；实际3.03s成功，HEAD `7f970b259e442ee562177cef5219d3a58b5a842f`、1165 Rust／910 Web内容清单和native Cargo cfg只读快照，plan SHA `f114545791e10c1e9ed483b24826c837ad27eb56f4d6d4708f988ff207bbc69e`。

原Sol仅必要小binding复核后裁决 `SOURCE_READY_PENDING_ROOT_LATEST_FRONT_AND_SLOT`，已交root/integration。没有重做已准基础设施或大件hash，未Cargo/bind/launch；slot仍pending/rootfalse。实际图仅engine lib双feature、j8/INC0、3case池、逐child及命令树10000ms，原296执行／297退出／298恢复／299发布／300绝对监督保持；必须三项指定stderr现金marker、唯一panic、child101与0pass1fail全部同时成立。root集中按Host闭包及实际最新front安排唯一窗，不以这张READY票称已red。


## 付款实际三首红与生产源码准入

付款3门真实session57906／WAIT0／chunk60f4e3，三指定现金marker精确命中，差额0对200／180／180。writer已亲读front、三stderr与case-result；front SHA `29c5308a9ca6c3c89072947fd39477ff118bb858a8bce8ea37b641ccb46c580b`。root亲核后授权最小生产，无Cargo由本writer执行。

02十文件生产经未实施者完整审查为APPROVED_SOURCE_ADMISSION，静态发现缺World stage类型reexport已修复复核；typed error保留原cause，详情见payment-production-02-independent-review.md。03仅在case04补真实raw GroupAccount前后相等，不改生产及首红01..03。patch SHA `6cabdb46eeb5caca7cb6175407c17a64e0cb75803defde58cd62f842d21d0881`，正等窄复核；完整原件02保留。尚无实际生产green，不报告付款完成。

后继只交development-main专职接收，保留其较新causal/WASM/Calendar内容，接收方审合成diff并由integration集中排唯一实际窗口；不再写旧integration。

03窄复核已通过并亲读（`payment-production-03-delta-independent-review.md`），同原02完整准入合并允许进入实际构建。最终生产源已存本地净commit `6fbcc324e3037e64082781bdaa56e1333d592366`，十文件799增84删；仅主题agents未跟踪，无其他产品WIP。已直交development-main receiver，integration已获精确16门建议，付款实际green仍待后继运行。


## 付款生产实际 green 与正式文档收口

原Tax16真实session31387／WAIT1／chunkc79d78为15green+1cash01恢复尾fixture FAIL：6/5本局冻结Calendar的DragonBoat休市日误step，不能称整case通过。最窄测试净commit `94ea2896` 仅1文件27增2删，从同局SSE Calendar求下一交易日，闭市逐自然日真endCivil且tick/day不变，开市后保真step+endCivil与原cash/gross-receipts/payments强尾。原Sol窄审通过；接收主干commit `99dc256e`。原失败保留。

Tax16r2实际session48124／WAIT0／chunk0aeadf、wall111.3062693089887s；固定镜像99dc256ea4a944f4c1635172e14b78e3c39728eb，tree226de8def03ac7955b667691d3352cf6a410330e，16/16green。writer亲读WAIT/inventory/launcher receipt、cash4全部stdout与stderr；每case唯一1pass0fail、wrapper0，cash01真实跨6/5至6/6，cash04真实raw账户回滚强尾通过。front SHA `ce62f654e6851d0b6cc6fea35119e63a24e1dbf502d0445c4d2e3f8fc4d56cd0` 已亲核；source manifest只核付款test小绑定SHA71f794cb…，无大产物hash/本writerCargo。四清场字段为空、scratchfalse、source/runner未变、lib原mtime/SHA恢复。

原未实施Sol有限actual复核与docs窄审 `APPROVED_PAYMENT_CASH4_LIMITED_ACTUAL_AND_DOCS`，已亲读完整报告 `payment-actual-green-01-limited-independent-review.md`。只关闭本固定镜像cash4待运行门，其他12项沿integration独审，不新称Tax01..05当前重跑、purse/部分生产收缴、多方案/transport、WASM或整树完成。

正式trading-rules两处尚待绿测已最小更新并独审，净commit `a9c2cc57`（1文档4增3删，patch SHA46efcb997c62036d269c94caf09812d5e8df0738b1c5c27152decfaa6c0cecaa）已直交唯一devmerge接收，无需新增Cargo。
