# sweep23：Task 11–13 历史独立复核与当前实现

## 阅读与基线

生产源码基线 `b76ece3`（审计工作树合入后的产品源码一致）。沿用本分片已读的 AGENTS.md/principles；没有产品修改、Git 写操作或测试执行。首次合并读取的终端输出截断后，已按文件连续补读剩余段至 EOF，未以搜索标题或旧测试结果替代全文。

| 连续全文阅读文件 | 行数 | EOF |
|---|---:|---:|
| `.omo/evidence/company-information-npc-intentions/task-11-review.md` | 332 | 332 |
| `.omo/evidence/company-information-npc-intentions/task-12-review.md` | 168 | 168 |
| `.omo/evidence/company-information-npc-intentions/task-13-review.md` | 141 | 141 |

共 641 行。原文 REJECT 与其文末转为 APPROVE 的复核均读完；下列旧测试数量只作历史状态，不作为 `b76ece3` 的实测验收。

## Task 11 逐章追踪

| 原文章节/行号 | 当前状态与代码反证 |
|---|---|
| 开头裁定 1；§1 执行证据 25 | cad7ac7 原拒绝是法源登记，不是地产处理器缺失；070ee58 的复核在本文件 265–332 完整覆盖。旧 20/711 测试结果不是本轮结果。 |
| §2 门禁三问 56 | 预售/交付/尾款处理器存在。`company/real_estate/presales.rs:164` 预售收款、`delivery.rs:37` 交付、`:132` 尾款；交付 `:69`–`:103` 同一分录冲合同负债/应收/收入并结转成本；未完工在 `:54` 拒绝。处理器不读市场或投资者现金。 |
| §3 K3 红线 90 | 永久停止/开放与闭合中断资本化 `company/real_estate/projects.rs:300`、`:313`；真实开发存货、减值、项目借款及还款分别在 land/development/impairment/debt_service。`real_estate/mod.rs:180` 先成功 post_batch 才推进事件 ID。零现金拒绝、重复交付等处理器守卫存在；不能由这些纯处理器直接核销四行业 Session 缺口 G36。 |
| §4 登记偏差 117 | 文件拆分/rhe_div 孪生/计数为历史工程裁定，不新列产品遗漏。VAT/预售许可等是明确获批简化。资本化 canonical 登记已补；CAS8 仍是法源或游戏假设登记债（见下），不是减值代码缺失。 |
| §5 原 CAS17 REJECT 147 | 已修复：`docs/company-accounting.md:121` 明确版本化假设；政策 fixture `:527` 有 `game-assumption-borrowing-capitalization`，覆盖行 `:686` 同时引用原 blocked source 与假设。原“补证前不得实现”禁令已被游戏假设路径替代，不重复开旧失败。 |
| §6 手算金样 212 | 原文窗口语义仍由 `projects.rs:281` 逐日计数、`:307` 首笔开发起、`:308` 完工止、`:315` 闭合长中断及 `:321` 开放中断判定实现；交付余量用差额递减 `:271`。本文没有重新实测旧数学全链。 |
| §7 非阻断观察 242；§8 拒绝范围 254 | CAS8 的 blocked 法源登记仍存在 `docs/company-accounting.md:59`、`:122`，而减值实现 `real_estate/impairment.rs:17` 已有；总账已在 S03-C1 及法源债说明纳入，不算新代码遗漏。suite/LOC 计数、防御性 UnknownProject 非待实现。 |
| §9 复验 265；§9.1 diff 273；§9.2 重跑 310；§9.3 终裁 323 | 当前 canonical 登记与 070ee58 修复方向相符，原主发现核销。旧隔离实测与清理记录属于历史证据；不重跑历史命令或还原旧工作树。 |

## Task 12 逐章追踪

| 原文章节/行号 | 当前状态与代码反证 |
|---|---|
| 开头 1；§1 隔离执行 13；§2 全读范围 29；证据对账 39 | consolidation 算法完整存在，历史统计与路径移动不是当前实现待办。 |
| §3 三问 47；Q1 49 | `accounting/consolidation/mod.rs:94` 纯 consolidate；group→coverage→aggregate→worksheet→minority 链存在。工作底稿不写成员账簿；少数权益/损益 `minority.rs:110`，单次半偶基点 `:132`；归母用总量减少数。单层、严格多数等六项简化已在 `docs/company-accounting.md:51` 和 fixture `:557` 登记。 |
| Q2 72；Q3 76 | 会计域镜像、只读函数与行业无跨层依赖；原 LOC 超线已获登记例外，不是产品实现缺口。任务13/26接线不能用算法测试核销，见G28。 |
| §4 F1 82 | 原登记缺失已修复：公司会计文档 `:49`–`:51` + fixture `:557`、`:668`。 |
| §4 F2 83 | 重复同成员对申报双倍抵销已修复：`consolidation/eliminate.rs:36` 在配对前检查双向计数及同向重复，`:51` Typed DuplicateIntercompanyDeclaration。不重开旧双倍抵销。 |
| §4 F3/F4/F5 84–86 | LOC 已选登记路径；提交文案计数是历史备注。五类错误专属测试原文明确非强制；算法守卫存在，不把非强制测试建议当代码遗漏。 |
| §4 F6 87 | **仍成立且已归 G28**：`eliminate.rs:116` 的 balance_entry 只检查双方金额相等、科目要素；随后直接按申报 amount 生成抵销，不比较双方账面余额。实际日终 Session 没有真实账簿→往来申报派生链。只指可配置API边界，不声称默认游戏已超额抵销。销售 `sale.rs:70`–`:90` 已检查invoice正数、cost≤invoice、unsold≤invoice；不能把销售自己的这些上界再报成未实现。 |
| §5 红线 90；§6 后续建议 110 | 固定集团 scope/无子公司 NotApplicable/图与持股/期间覆盖/工作底稿与现金语义已实现；canonical 登记已补。最终交付接线仍在G28。 |
| 复验 120；F2修复 127；F1/F3修复 134；重跑 143；处置状态 153；裁定 164 | 原文明确F1/F2已修、F3登记、F4备注、F5非强制、F6观察。本次逐一追当前代码后仍采用这个区分，F6交付链纳入G28；历史APPROVE不能核销其明示的下游契约。 |

## Task 13 逐章追踪

| 原文章节/行号 | 当前状态与代码反证 |
|---|---|
| 开头 1；§一 隔离重跑 11 | 原F1/F2/F3由ceadac7+52b0d16修复后APPROVE；本轮不使用旧791/793测试计数宣称新基线验收。 |
| §二/Q1 24/26，原F1 30 | 原“更正只在一次调用映射、泄漏次年利润”已修：`closing/mod.rs:99` 持久 RestatementRegister、`:270`–`:274` 累积更正；`:367` 所有单体后续生成取本scope底稿；`closing/save.rs:21`、`:32`、`:45` 保留底稿。`reports/window.rs:180` 按有效期间决定损益窗口，实际现金仍按实际期间。 |
| Q2/F2 36/40 | 七项报表与结账简化已登记 `docs/company-accounting.md:137`、fixture `:606`，不用再要求实现年末结转/合并更正/OCI等已获批简化。 |
| Q3/F3 43/48 | 附注溢出不再吞错：`reports/notes.rs:171` 返回Result，`:193` `closing.sub(movement)?`；缺运动桶=0是合法投影，不是静默吞溢出。 |
| §三 BusinessKind债 51 | 报表仍消费科目/流量而非BusinessKind；已获准顺延的标签迁移不是本轮漏实现。依据 `reports/mod.rs:187`、`window.rs:180`。 |
| §四 验收核对 59 | 四行业与合并五产物纯生成已有 `reports/mod.rs:137`、`:187`；实际合并公开缺口见G28，四行业Session装配见G36。单体月年封账/快照更正/历史版本、缺历史Typed Unavailable、报表守卫存在。原红线拒绝原子性存在后置失败候选，见下一节，不用成功金样覆盖。 |
| §五 转APPROVE条件 79；§六 清理 86 | 旧三个修复项确已反映在当前代码；历史命令/临时worktree清理不是当前产品工作。 |
| 二次复验 92；F1 97；F2 113；F3 119；重跑 123；其他 133 | 持久底稿、费用/实际现金双口径、canonical登记及附注错误传播反证均成立。原文101明确合并更正不支持且当时仅测试构造；普通固定集团公开链仍属G28。 |

## 新候选 S23-C1：更正后置报表失败可能留下部分提交

**候选状态：需主审裁定；已证明代码顺序与可达错误，未运行探针，不宣称默认日终已触发。**

- 原文 `task-13-review.md:69`：已封月份经带更正来源的当前开放期间调整凭证入账，并“拒绝后账套零改动”；当前更正函数也声明原子过账。它要求的不只是旧F1跨期利润正确。
- 当前 `accounting/closing/mod.rs:270` 先直接修改调用者 `Books`，`:273` 修改持久 restatements，`:282` 再调用可失败 `generate_with`，`:292` 通过 `?` 返回错误。`generate_with` 在`:385`调用纯报表生成后还于`:396`校验；函数没有影子提交/回滚。
- 代码可达边界：对已用Industrial成功结账的真实Books调用public `correct`，同样传合法前向开放期间的平衡调整分录，但这次传 `IndustryPresentation::Bank`。`post_batch`不验证列报行业，会成功。之后 `reports/mod.rs:198` 按输入行业归类；`reports/notes.rs:99` Bank只取银行表，`:125` 检查账套所有科目，`:127` 对工商表中未在银行表的科目返回 `UnclassifiedAccount`。这时 journal/ledger与底稿已改变、report版本未store。错误行业本应拒绝，问题是拒绝时已落下调整。
- 现有 `tests/industry_reports/failures/rejects.rs:164` 的 correction_guards_reject 只覆盖未封目标、无前版、调整倒退与底座已封期间；`:279` 的状态不变断言在前向校验失败路径，不触及成功post后的归类/勾稽失败。
- 没有在总账G28/G35/G36中找到这项独立边界。建议主审确认这是当前公共ClosingEngine错误原子性契约还是只列查询层API候选；若收录，后续用代表性短测断言返回Err时Books及ClosingEngine序列化事实都不变。不得扩为更改CAS会计语义或新增兼容旧档。

## 已登记缺口与反证汇总

- G28：普通固定集团的真实日终合并→关闭版本→PublicLibrary发布仍没有生产链。`session/disclosures.rs:166`只取单家公司账簿，`:186`固定Standalone；等额往来账面上界仍漏。原Task12 F6应保留在同一条，避免重复。
- G36：四行业纯处理器/报表已经存在，不能代替自定义Session装配与公共报告查询。地产默认标的不是原Task11必要条件，不凭此新增默认地产股需求。
- CAS8来源仍blocked而实现已在：属于总账既有S03-C1法源债；CAS17/CAS33/report-closing原登记缺失则都有当前反证，应核销旧失败。
- 没有新增“未实现利润销售cost/unsold上界”、“重复成员对双倍抵销”、“更正跨年重复利润”、“附注溢出吞错”、“未登记七项简化”等已修问题。

本轮只有静态源码核对；S23-C1保留为候选，不将历史测试与APPROVE当作新测试结果。
