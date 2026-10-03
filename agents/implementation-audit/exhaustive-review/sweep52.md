# sweep52：accounting OOP 实施与复核记录全文回查

## 基线与阅读

连续全文读完 `agents/oop-refactor-implementation/README.md` 1–7 行、`domain/accounting-result.md` 1–65 行、`domain/accounting-review.md` 1–126 行，合计198行，均到 EOF。已读根 AGENTS.md、docs/principles.md；本路径未发现更深 AGENTS.md。源码基线 b76ece3，当前合并树在 apps/packages/scripts/.github 与该基线无 diff。只新增本记录，未修改产品、未 Git 写、未测试或编译。

本组原文的目标是九个 owner 动作，保持既有行为；不能用“方法化完成”核销正式公司计划的日终经营/合并披露缺口。工作记录的静态 APPROVE、历史短测与内容SHA都是其特定版本证据，不冒充本轮验收。

## README 全条款族

| 原文位置 | 判定 | 依据与边界 |
|---|---|---|
| 1–3：128动作汇总/owner/方法/caller台账 | 导航与汇总承诺，本批核九个accounting动作 | 生产方法/caller见下表；不据plan.json状态宣称128项均本轮复验。 |
| 5：分支、短编译/类型检查、不跑复杂回归/中文 | 历史实施范围；当前审计遵守 | 不是当前产品缺少E2E或长期矩阵的独立功能要求，当前发布build-only由ADR-0028决定。 |
| 7：保留原调查、冻结清单、独立总审、本地提交不push | 工作证据与协作，不产生runtime路径 | 本批不推送、不重写原SHA/旧调查；最终验证历史范围需单独报告。 |

## 九个动作逐一核 caller→owner→consumer

| 动作、实施原文/复核原文 | 当前调用闭环 | 判定/剩余边界 |
|---|---|---|
| N05，result:19、review:63 | `reports/consolidated_window.rs:147` Builder调用→`reports/window.rs:257` Accumulator.add_current_worksheet→`:274` private bucket_all→`:279`依次closing/movement/quarter/ytd；`reports/mod.rs:213`消费者生成报表 | owner已接；借贷符号/checked顺序/现金和历史桶不摄入已保留。Builder是临时构建owner，错误时丢弃，不因局部桶先写后报错新增持久错误G。原文“自由apply_worksheet删除”仅指reports旧helper，`consolidation/eliminate.rs:162`同名抵销算法是不同层职责，不能误判重复实现残留。 |
| N06，result:20、review:64/73–78 | Industrial `sales.rs:76`→VatPolicy.output_vat_on（`accounting/tax.rs:99`）；`purchasing.rs:76`→split_input_vat（`:107`）；`expenses.rs:267`→IncomeTaxPolicy.compute（`:153`）；公开tax free API `:75`/`:90`/`:142`只委托 | 税算法owner与生产caller已接；两次half-even/只读亏损池保留。税率法源仍blocked，不能称真实税法验收。所得税处理器已接行业方法，但Session日终调度缺口G35仍存在，不因policy方法化核销。 |
| R2-N01，result:21、review:65 | `closing/mod.rs:239`correct→`:270`Books.post_batch→`:274`RestatementRegister.record_correction→`:282`generate_with→store；`closing/save.rs:32`save_rows、`:45`from_rows | Register真实owner，确定性行投影已有。correct的generate失败可发生在账簿/重述登记成功之后，原文明确保留，不是这批遗漏的原子性重构任务；Books.post_batch的原子保证与整个correct的原子保证须分开。 |
| R2-N02，result:22、review:66/79 | FixedAssetRegister方法 `fixed_assets.rs:193`/`:202`/`:213`/`:226`查Entry并委托→Entry`:36`/`:51`/`:64`/`:84`；Industrial `capex.rs:68` preview→post→`:94` apply，减值`:107`validate→post→`:123`apply | Entry迁移已接，寿命月/残值/折旧/减值checked规则未改变。生产经营周期未调depreciate_month属于G35；不能因为capex直接API存在宣称日终闭环。 |
| R2-N03，result:23、review:67/80 | InventoryLedger `inventory.rs:169`receipt→ItemState`:70`；preview_issue`:102`→apply_issue`:134`；Industrial purchase `purchasing.rs:114` receipt、sales `sales.rs:107`issue、production `production.rs:117`issue及`:120`receipt | State非空壳，真实caller已有。receipt金额溢出前quantity已写（`:94`/`:98`），原文与新增测试显式保留。该旧失败语义见候选反证，不把本次保行为记录视为承诺改成原子。 |
| R2-N04，result:24、review:68 | `consolidation/eliminate.rs:79`→IntercompanySale.validate_for（`sale.rs:30`）→ValidatedIntercompanySale.to_worksheet_entry（`:113`），私有借用validated字段；consolidate生成worksheet后由reports消费 | sale验证/利润折算唯一生产主体；没有为OOP新增第二抵销算法。IntercompanyBalance账面上界校验不属于sale方法化完成证明，G28余额申报派生/上界仍需专题修复。 |
| R2-N06，result:25、review:69 | `balance_sheet.rs:187`→EquityPresentation.from_closing（`:251`），`:237`→from_prior（`:284`）；`:316`lines、`:327`total_equity、`:335`equity_to_parent被本报表生成消费 | 权益列报owner已接；比较期缺prior_split省略权益是既有简化，不要求顺手造比较数据。少数比例与舍入仍由原合并层负责，不因EquityPresentation无比例算法判漏。 |
| R2-N07，result:26、review:70 | `reports/mod.rs:199`单体、`:214`合并→ReportClassification.from_industries（`notes.rs:117`，`:125`覆盖校验）→`:228`balance_sheet、`:229`income、`:232`notes | 分类有效对象已在真实报表caller构造；map private；额外映射/重复同目标行为仍有。四行业map覆盖不等于四行业Session装配/封账/披露都完成，G36不能核销。 |
| R2-N08，result:27、review:71 | `reports/mod.rs:213`→consolidated_window::consolidated（`:52`）→Builder.new/scan_members（`:57`/`:58`）→consolidate（`:59`）→apply（`:60`）→finish（`:61`、`:151`）→StatementWindows被equity/notes消费 | Builder真实持中间事实/历史权益；输入与BTree遍历/worksheet→prior_split顺序保留。合并报表内核不是生产日终固定集团公开闭环，G28仍成立。 |

## accounting-result 其余全部章节

| 全章/原文 | 判定与代码/正式证据 |
|---|---|
| 标题/范围/依据1–13 | 保行为重构，13 own文件；金额AccountingAmount i128分、存货件、寿命月、股数股，未新增股东现金流。`accounting/tax.rs:3`及`:9`沿用VAT/CIT法源blocked与全额DTA游戏假设；`docs/company-accounting.md:51`固定控制/单层等简化、`:56`移动加权平均、`:145`合并工作底稿当期归属。法源债不能直接转为缺税模块。 |
| 逐动作15–27 | 九动作全部在上表逐行追caller/owner/consumer，无空壳仅测试接缝被误称生产完成。 |
| 验证29–45 | 第31行实施者未跑与第44行root后来跑是不同执行人/时间，不矛盾。十三短测/formatter/diff-check历史结果本轮未重跑；建议accounting::/集成二进制入口是验证指引，不承诺产品周期新功能。原文第45行明确不宣称全回归。 |
| 源清单47–65 | 13路径现存，上表覆盖全部文件，包括closing/save与income/mod/notes调用链。没有新增源码文件是历史范围事实，不要求为owner新增物理文件。 |

## accounting-review 全章节与门禁

| 全章/原文 | 当前判定与边界 |
|---|---|
| 元数据1–7、依据9–28 | 身份与完整旧diff范围已记录；本轮没有重新审b89afb3整批diff，不把原APPROVE当当前全仓无缺口证明。13文件全部核当前production连接。 |
| 门禁1 32–44：单位/股东资金流/官方依据 | 当前owner代码仍相同单位；不因会计company金额可大于Money而要求转换i128→i64。已知CAS与税法blocked被正式`company-accounting.md:245`保持；没有新增法源规则，此次未联网。 |
| 门禁2 46–57：最小范围/可选内聚/无第二账本/兼容API | 九owner都有生产调用，free税API纯委托、ReportClassification有效对象现存；owner方法化本身为架构优化，不追加产品功能；旧public API保留不是存档兼容迁移器或第二权威账本。 |
| 门禁3 59–80：九动作细节/Industrial caller | 全部映射见上表。显式保留的部分写入/首错顺序不改成未实施功能要求。 |
| 测试局限82–94 | “同年loss排序/全部错误排列/恶意serde不穷举”是明确验证局限；没有证据发现具体遗漏接受了错误状态就不单凭未穷举新建G。四行业chart跨层仍需gold/公开财报集成验证，当前G36是生产装配缺口，不能用分类测试核销。实施先后不能仅由最终diff证明，未冒称TDD红灯本轮复现。 |
| 最终绑定96–126（含完整13项SHA表） | 只适用于列出的旧工作树blob；HEAD当时仍b89afb3的说明避免把HEAD冒充未提交代码版本。当前产品基线b76ece3，未重新计算原SHA或改历史记录；后续变化必须另复核，本次只核生产owner路径不升级历史测试覆盖。 |

## 候选反证、残余与总账建议

1. **“accounting仅测试调用、owner未迁真实caller”被反证。** 九动作真实库/行业caller全部已接，未发现新增漏接owner。正式会话闭环仍G28/G35/G36，不重复新增。
2. **“Inventory receipt错误应零变更但仍部分写”暂不新增G。** 原result:23/review:67明确锁定旧quantity→cost写序；源码`inventory.rs:94`确有该行为，Industrial purchase先Books过账再receipt（`purchasing.rs:103`/`:115`）。这不是新增正常业务功能未实现；正式公司计划:86的原子承诺针对Journal整批，不可直接扩大成所有子账API全域事务。极限i128错误的跨owner一致性仍可另作风险专项验证，不能将本次保行为OOP强行要求改语义。Session日结整体失败边界也须与直接库API分开。
3. **“Closing correct生成失败有写入所以原子更正未实现”暂不新增G。** 原result:21/review:65明确保留该范围；Books.post_batch成功原子与generate失败后已提交事实是两层。未追到正式全域更正事务独立承诺，不把推测产品方向变G。
4. **“full regression/官方税法完整验证完成”没有根据。** 两记录明确静态/短测局限及blocked法源；本轮未执行验证。该项继续证据债，不虚构当前测试失败或新模块任务。

新增确认缺口/候选均0；旧G28/G35/G36保持，Q项无分类变化。记录中的明确后续主要是验证入口和范围诚实性，没有遗漏实现的新正式产品承诺。
