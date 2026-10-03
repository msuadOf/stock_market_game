# Sweep 22：历史恢复回执、Task 1 与 Task 10 独立复核

- 产品基线 `b76ece3`；当前审计合入 HEAD `4ad5a2e` 的生产文件与其相同。本轮沿用已读 AGENTS/principles/ADR-0016 及 Q12 后续决定，不改变产品语义。
- 连续全文读至 EOF：`.omo/evidence/company-information-npc-intentions/notepad-recovery.md` 63 行、`task-1-review.md` 27 行、`task-10-review.md` 253 行，共 343 行；工具返回未截断这三份文档。
- 仅新增本工作记录，未运行测试、编译或长验收。下面的历史测试数字是旧证据，不是当前运行结果；没有把历史 REJECT/未完成直接转为现行缺口。

## Notepad Recovery Receipt 全章节

| 原文章节/行号 | 条款族与现行判断 |
|---|---|
| 标题/日期/Scope 1–4 | 2026-09-12 notepad 修复回执，只涉及 learnings/issues 与回执，不是产品功能计划 |
| Recovery method 6–16 | Git blobs 精确恢复、基线行数/hash、旧乱码保留；为证据来源声明，不新增运行时需求；不把2026-09-12 hash要求套到之后持续追加文件 |
| Appended sources 18–35 | Task27–32 往返/报告/绑定/宿主/验收材料来源；task31“unresolved issues/no completion”只说明当时，当前必须重查对应宿主，不直接创建缺口 |
| Result 37–46 | 恢复后1227/1195行及hash/UTF-8/EOF/标题完整性，是当时文件状态；以后追加导致hash变化不是现行失败 |
| Unresolved loss and incident 48–58 | 未完整留存 Tasks27–31 post-HEAD 原文属于历史证据遗失；没有凭旧报告补造；append-only notepad 禁止整文件覆盖为持续工作纪律，本轮未碰 notepad |
| Scope receipt 60–63 | 不改产品/测试/计划/Git；范围回执不构成产品完整性声明 |

回执提到的当前产品能力有明确入口反证：`session.rs:2039` public_report_by_id、`session.rs:2100`自然日日终经营/封账/披露、`session.rs:2801` CivilClock 恢复，当前公共报告与恢复不能因历史 Task29/30 build限制判未实现。此回执不详述Task31发现内容，故不能由31–32行猜测 Tauri 现行缺口；完整宿主审计归相应 sweep。

## Task 1 Review 全章节

| 原文段/行号 | 当前代码/后续决定与判定 |
|---|---|
| Metadata 1–5 | 2026-09-10 add-only基线提交0a77d8f独立复核；只证明那批3文件，不证明当前完整量价验收 |
| Gate1 7–8 | 旧matrix/compressed股票/费用/T+1时钟参数；当前 `examples/baseline_fixture.rs:243` proposed_defaults，300 matrix硬固定、388 compressed固定测试仍在；`scripts/simulation/baseline-run.mjs:35`旧SCENARIOS仍用于密封before解析，文件头5行明确 CLI 不再允许重跑before。旧共享V/旧行为参数不能继续要求当前产品保留 |
| Gate2 10–11 | 最小三文件、无读存档/旧格式；该任务局部范围不能禁止之后新格式恢复；本轮不从文件数量或行数推导当前功能失败 |
| Gate3 13–14 | 当时22 Node/5 Rust及负控覆盖为历史通过记录；当前 `baseline-run.mjs:78` seed拒重，1081 malformed JSON显式错，checkpoint/determinism等有新实现；未运行当前测试，不能报告旧数字全绿 |
| Gate4 16–17 | 非零engine_error_events显式记录、manifest hash及工具不可用诚实性；当前 `baseline-run.mjs:462,494`旧报告错误数字保留和十进制字符串校验，测试 `baseline-run.test.mjs:1568,1596`对应；旧55–98错误数不作为现行必须产生的错误 |
| Nits1 19–20 | BASELINE_FIXTURE_TIMEOUT_MS非法值fallback旧问题被取代；当前runner没有该变量，`baseline-run.mjs:140,147` deadline输入有整数/正值/300000上界校验 |
| Nits2 21 | 旧fakeExec dead corepack分支是旧测试维护观察，无当前产品功能承诺 |
| Nits3 22 | stderr截断的信息项本来不是阻塞；当前采集路径1066–1081失败显式抛错，不能因历史截断要求全部stderr进入产品UI |
| Nits4 23 | invalid JSON stdout当时缺直接单测为测试建议，不是代码漏实现；当前1081明确JSON解析失败，不升级新G |
| Pre-existing/VERDICT 25–27 | 旧clippy与APPROVE只指旧提交；不当作当前构建结果或可跳过当前独立复核依据 |

现行 runner 已另有并发、共享deadline、密封before、源指纹/断点与清理规则，旧基线APPROVE不能替代本轮K7 G39等现行审计。`baseline-run.mjs:24–26`硬上限/清理保留、66–67并发参数，未执行长任务。

## Task 10 Review 全章节及条款族

| 原文段/行号 | 当前代码及判断 |
|---|---|
| 状态历史 1–23 | REJECT唯一原因是D1–D5登记缺失，e8210bb已翻APPROVE；当前docs/fixture仍有登记，不能重复报“保险假设未登记” |
| §0隔离协议 27–37 | 当时避免脏主树cargo及不碰产品；工作纪律/历史环境，不是当前运行链需求 |
| §1命令结果 39–46 | 17保险/728全量/check/clippy历史结果；本轮不复用为当前测试通过 |
| §2问1法源 48–59 | CAS25/CAS30锚点；当前 `docs/company-accounting.md:87–111`保险行与假设分开，法源沿用，未联网重核 |
| 保费/服务/赔案/重估红线 60–81 | `insurance/premium.rs:62,174`建立/收款、`service_release.rs:24`释放、`claims.rs:111,164`发生与付款、`remeasure.rs:22`重估已有；`operations/insurance.rs:54,71,75,76`日常经营消费，不能说全部只有测试内核；完整会话装配仍G36 |
| 整数/边界/不支持 82–90 | `insurance/csm.rs:20,37,56`整数半偶/贴现/单元释放；`insurance/mod.rs:82`UnsupportedContract；`tests/insurance_accounting/failures/guards.rs:79`10_001上界测试已有。真实复杂合同仍明确不支持 |
| post_with_commit/存档/解耦 91–99 | `insurance/mod.rs:160`提交入口、claims140/194与service_release97消费；groups457/489 serde owner投影已演进；历史旧存档“可反序列化”被ADR-0016:115及现行schema v2门取代，不要求迁移器 |
| §2问2必要性 104–111 | 旧22文件、250行天花板/纯行数属于当时改动验收；当前OOP拆分与文件长度不能单独证明缺功能 |
| §2问3边界 113–117 | 10_001已补，不保留历史遗漏 |
| 跨chart警示 118–122 | 后续完整合并必须按行业列报分类、不按跨chart原始code合并；当前仍见真实缺口，见S22-N1 |
| rhe_div孪生/复杂度 123–125 | 复用数学孪生为非阻塞维护观察；不从重复函数本身新增未实现需求 |
| §3事实链 127–147 | 未登记的60c7c00历史原因已被复审覆盖，不能当现行事实 |
| §3 D1–D5 149–155 | 五项语义裁定游戏可接受，当前 `docs/company-accounting.md:107–111`完整登记；不把获取现金流摊销/利率重估缺失重新报G |
| §3修复路径 157–165 | fixture假设条目+coverage+docs+可选上界测试已实现：fixture542、679–681，docs93–110，guards79 |
| §4通过项 167–173 | 历史整数/原子/金样等通过，只作历史核查线索；不宣称当前729全绿 |
| §5证据 175–184 | Windows临时路径和notepad来源是历史来源，不要求本环境复制或伪造旧日志 |
| Re-verification状态 188–192 | 最终APPROVE覆盖初审REJECT；现行登记仍存在 |
| 隔离与主树 194–203 | 历史task14 zombie与未提交generated不等于当前代码残留或bug |
| Diff核验 205–232 | 三文件修复、D1–D5、coverage三行、10001边界已核当前实体，见以上行号 |
| 复跑结果 234–241 | 5manifest/18保险/729全量历史数字，本轮没跑 |
| 三问终评及结论 243–253 | 原批次门禁完成；后续跨行业真实生产消费仍要另核，不因APPROVE提前核销G28/G36 |

## S22-N1：跨行业合并保留原始科目代码，保险与其他行业的完整报表归类冲突

原文 `task-10-review.md:118–122` 明确：1122保险应收保费与地产应收款、2501保险LRC与地产长期借款属于各行业命名空间；“任务13/33报表生成与合并必须经各行业 presentation_lines 分类层取数，不得跨 chart 原始科目码合并”。现行 ADR-0016:97–101 完整交付四行业及合并；`docs/company-accounting.md:51` D1–D6合并简化未批准将保险/地产混合集团拒绝或混同科目。

生产底层 `accounting/consolidation/aggregate.rs:73–110` 按 LedgerAccountId 原始code加总，121仅验element/is_cash/is_contra，允许名称不同。保险 `reports/insurance.rs:22–24` 1122=应收保费/2501=LRC，地产 `reports/real_estate.rs:21,27`1122=应收款/2501=长期借款；它们同要素非cash/contra可通过该校验并混同余额。`reports/consolidated_window.rs:97–108` defs取首成员同code定义并用该defs累计全部成员分录，未保留成员行业键。

完整五产物 `reports/mod.rs:202–217` 收集所有成员行业、构造合并窗口、把全量行业归类并到一个code-map。`reports/notes.rs:79–85` 不同target会 DuplicateClassification：保险1122映射InsuranceReceivables(`insurance.rs:93–94`)、地产1122映射Receivables(`real_estate.rs:101`)；2501的保险/长期借款映射同样冲突。归类全量合并发生在无余额过滤之前，因此只要保险与地产同时出现就会失败，不要求两个成员当期恰好都使用冲突科目。保险+工商也有1122同类冲突。

**反证与范围**：错误为显式拒绝，不是目前公开库已经悄悄发布错表；默认session不装集团且仅Industrial，G28公开接线未完成，故不声称默认生产局已触发。底层混合工商+银行测试 `tests/consolidation/gold.rs:34,109–116`只调consolidate及少数股东拆分，不是完整generate_report_set；`reports/notes.rs:276`逐行业/重复同一种行业测试不是跨行业联合分类反证。此前合并简化无跨行业科目冲突豁免。这个遗漏在G28接上调用链后仍会阻塞混合集团五产物，应归入G28子项，或单列内核跨层语义遗漏。

建议验证：少量同期间合法保险+地产成员完整generate_report_set，分别保留InsuranceReceivables、Receivables、InsuranceContractLiabilities、LongTermBorrowings及现金/权益/损益勾稽；增加保险+工商与仅同业集团对照。不得仅放宽 DuplicateClassification 或按首成员名称归类以掩盖余额混同。本轮未运行验证。

## 未新增的候选

- Task10初审登记缺失、10_001缺测试均已有后续修复反证。
- 获取现金流摊销/贴现率变动、RA显式输入、简单贴现/亏损列报为批准并登记的D1–D5，不视作当前漏实现。
- Task1旧参数、before重跑、旧存档兼容被现行决定取代；严格失败上下文与源证据诚实性继续检验，不能保留旧实现形状作为必须。
- Notepad遗失原文属于证据完整性债，不是缺少某个产品功能的直接证明。
