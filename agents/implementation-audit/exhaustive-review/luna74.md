# Session roots 文档与生产接线独立复核

## 范围与方法

- 复核产品基线 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`（本工作树对应产品 `08e4fc7` 的 merge 内容）；依任务只读审查 `roots-callers.md`、`roots-review.md`、`roots.md`，本次只新增本记录。
- 开工已读仓库 `AGENTS.md` 与 `docs/principles.md`。复核依据还包括当前 `decision_chain/roots.rs`、`decision_chain.rs`、`decision_chain/quote.rs`、`decision_chain/lifecycle.rs`、`plan_chain_candidates.rs` 与 `plans/allocation.rs` 的实际生产实现和caller，以及既有实现审计总账对 G16/G38 的原定义。
- 三文件共172行：`roots-callers.md` 9行、`roots-review.md` 121行、`roots.md` 42行。逐行覆盖矩阵如下；引用源码行号均为本次工作树现行行号。
- 未运行测试、编译、性能/长验收、官方规则联网核对或Git写操作。大A判断仅确认这批重构/短修没有改动既有交易约束；不将旧审查者的运行记录冒充本次执行证据。

## 全文逐章矩阵

| 文档与原文行号 | 逐条复核及现行证据 | 结论 |
|---|---|---|
| `roots-callers.md:1–2` 标题与范围 | 记录限定 `quote.rs`、`lifecycle.rs` 两 caller 文件及 getter 接线；当前仍是 getter 调用而非业务策略迁移。 | 范围描述准确。 |
| `roots-callers.md:3–4` 前置阅读与实现来源 | 声称阅读两个 caller 与 `ParentOrderPlan` getter；当前对应母单 getter仍在私有类型上提供并由两处caller使用。 | 局部阅读声明可与目标相符，不代表全批diff审查。 |
| `roots-callers.md:5–6` 10个 E0616 与五个字段迁移 | `quote.rs:165–172` 使用 `linked_plan_id()`、`active_child_order_id()`、`limit_price()`、`active_child_remaining_qty()`、`child_qty()`；`lifecycle.rs:352–354,486–495` 使用相关 getter。字段读取迁移实际到达生产调用点。 | 历史诊断已修，不可将初始E0616再次计为现行编译缺陷。 |
| `roots-callers.md:7` 交易语义未变 | getter返回事实并未更改调用处分支、数量、价格或撤单时序；本轮未查阅新的交易所材料，也没有重做规则依据核验。 | 范围内静态成立；交易制度依据仍承接正式文档，不声称重新核验。 |
| `roots-callers.md:8–9` 格式/扫描及验收限制 | 记载局部`rustfmt`与字段搜索；明确未运行Cargo/test，由上级集中验证。 | 诚实区分worker局部工作与全批验证；本次也未重跑。 |
| `roots-review.md:1–7` 元数据、范围结论 | 日期/复核者/baseline及“静态复核通过”是历史复核记录。它明确不表示Cargo、短测、回归或长期验收通过。 | 结论边界有声明；不能仅凭签署结论推定本次重演其完整diff审查。 |
| `roots-review.md:9–25` 范围与方法 | 文中区分十个主审文件、两份新增文件全文、长文件机械比较、personal-state接线检查和其他worker职责。 | 审查范围限定清楚；其“完整”只指记录内明示范围。 |
| `roots-review.md:27–40` 大A语义 | 列出Money分、数量股、bp、整手、零股、T+1、撤单、申报量、价格笼与费用保持；CandidateTargetProposal没有冒充成交。未发现本次getter变更改写这些条件。 | 静态范围内无语义漂移；现行规则有效性及费用表重验不在此记录证明范围。 |
| `roots-review.md:42–54` 必要性/最小范围与R02 | root、coordinator、lifecycle、route、proposal分别承担所述职责；R02把新增账户校验撤回。当前`roots.rs:18–29`仍是原账户clone与`plans.clone()`，未见已撤校验复入。 | owner职责接线仍存在；R02关闭成立。全历史计划复制另见G16，不能由职责分离宣告解决。 |
| `roots-review.md:56–83` 边界、测试与静态比较 | 记录对观察/反馈次序、固定资源与当前PlanBook、三route-map生命周期、proposal换算顺序、54个旧test及新短测的核验结论。当前`plan_chain_candidates.rs:304–326,371–383`显示capture、observe、回传及安装真实连接；`adaptive.rs`的固定观察调用在既有相关复核中有定位。 | 不能据此推导全部交易状态空间已运行验证；文内把静态复核和未运行测试区分开。 |
| `roots-review.md:85–98` R01–R04与增量修复 | R01多余append、R02新增校验、R03错误location、R04无caller wrapper清理均给出修复/复核轨迹。当前route接线及getter消费可见；无证据表明这些初始问题仍在指定路径。 | 旧发现按当前源码均有修复反证；不得重复报已关闭故障。 |
| `roots-review.md:100–118` 文件绑定 | SHA与行数绑定的是签署时的十个文件内容；其文本说明变更需增量复核。 | 是历史签署边界，不是性能或业务完成证明；本任务不重算全表hash。 |
| `roots-review.md:120–121` 集中运行义务 | 清楚说明待集中编译/短测，且新增测试没有实际红绿运行证据。后续实施总记录另载了集中执行结果。 | 本记录不运行验证，也不把此处当作产品行为缺陷。 |
| `roots.md:1–7` 范围与依据 | 以A01、R2动作和指定caller为范围，明确personal state及计划/执行owner另有实施者；交易依据沿用正式登记。 | 不应扩大为全计划产品验收；正式规则文档仍优先。 |
| `roots.md:9–11` Observation | 一次封存MarketView、路径、技术结果、时点和曝光，tick/minute/phase与账户校验仍在捕获适配器。 | 观察与校验边界说明与生产接线相符；不等于删掉外部校验。 |
| `roots.md:12` RootReadContext | 描述COW账户及只读PlanBook，无GameSession/市场/历史/个人map/游标。现行`roots.rs:6–14`字段确实相符；但capture在`:25`执行`session.state.plans.clone()`。 | 权限/持有面描述准确，但不能把“只读”解读成没有历史复制；G16仍未核销。 |
| `roots.md:13–14` InstitutionDecisionRoot与Coordinator | `roots.rs:187–221`持单账户personal并输出typed batch/diagnostics；`plan_chain_candidates.rs:304–333,371–383`真实创建worker、观察、回传并安装个人状态。 | 非空壳、非无caller owner；真实生产调用存在。 |
| `roots.md:15–17` lifecycle、三route-map、proposal | lifecycle保留固定资源/当前candidate事实；route三个状态map分开消费；proposal负责意向换算，不代替成交。 | 对owner职责边界的声明成立；G38预算类别输入不在这些迁移中补齐。 |
| `roots.md:19–20` 业务条件与事务 | 声称既有风险门槛、个人暂停恢复、交易数量单位、T+1、价格/费用、P9及日终门禁不变。就指定caller短修与当前代码核对未发现改变。 | 仅能结论“该重构未显示变更”；不等价于所有条款已完整实现。 |
| `roots.md:21–23` 测试/静态核对 | worker明确未运行cargo/产品测试；列新增用例意图，集中验证与完整diff复核交上级。 | 历史测试清单不是本次运行记录，也不代表每个需求都行为覆盖。 |
| `roots.md:25–34` 测试过滤器及保留守卫 | root-coordinator、route、clock、fixed resource/current plan与既有交易守卫逐项陈述。当前查看的生产路径支持其调用链存在；本次没有审测试体和运行结果。 | 测试存在/计划执行需与产品能力和本次执行状态区分。 |
| `roots.md:36–38` 复核修正 | 记载append、route location、getter与其他接口编译修正，并明确由独立caller工作者处理三个文件。 | 当前caller的getter调用提供修复反证；初始缺陷不应复开。 |
| `roots.md:40` R02与`cfg(test)` | 记载普通COW clone、不改旧错误接受集，测试专用wrapper受cfg限制。与当前RootReadContext账户clone一致。 | 修正可核；与PlanBook复制性能问题是不同对象/契约。 |
| `roots.md:42` 状态 | 当时写“等待root最终编译/短测及独立复核”，属于当时交接快照。后续`roots-review.md`与`session/implementation.md`更新状态和证据。 | 不以过时“等待”断言当前未完成，也不将旧记录作为本次验证。 |

## 原文结论复核及现行候选

| 原结论/候选 | 现行原文代码与调用链 | 复核结论及范围 |
|---|---|---|
| G16：普通tick不应随多年历史线性复制 | `packages/engine/src/session/decision_chain/roots.rs:11` `plans: PlanBook`；`:25` `plans: session.state.plans.clone()`；`packages/engine/src/session/plan_chain_candidates.rs:304` 每个剩余账户批次capture一次，再由`:319–326`多个worker共享同一`Arc`。 | **G16仍成立，不可因RootReadContext/Arc owner抽取而核销。** `Arc`避免每个worker再复制，但`:25`整本`PlanBook` clone仍按历史体量发生。正式目标是普通tick不随多年历史线性复制；缺口是所有权/复制成本，不是删除历史。未测量，故不声称实际wall-time瓶颈或幅度。 |
| G38：本人预算要区分已有计划续行与新机会 | `decision_chain.rs:968–979`的卖预算请求显式`ExistingPlan`，`:1232–1243`买预算请求也显式`ExistingPlan`；`plans/allocation.rs:116–126`按`RiskReduction`、`ExistingPlan`、`NewOpportunity`排序。 | **G38仍成立，不可由root/lifecycle owner迁移或分配器模块存在核销。** 当前生产请求没有把新机会分类带入；范围是同一账户的软预算类别，不是跨账户/跨股票撮合优先级。买卖现有请求的体验字段默认值不是本候选依据，也不另造重复缺口。 |
| 有无由三份文档暴露的新独立G | R01–R04、E0616 getter、个人状态take/install与worker结果回交均有当前实现调用点或已记录修复反证；文本提出的A股单位/行情边界未见随访问器改动漂移。 | 未确认G16/G38以外的新现行产品缺口。对正式需求的整体验收、实际交易制度外部复核和性能测量不在本轮范围。 |

## 收束

三份文档的条款/章节已覆盖至EOF。roots实施与短修的owner边界和实际caller大体有现行源码支撑；历史修复结论没有发现应重开的残留。G16的完整PlanBook复制与G38的新机会生产分类缺失仍是独立的现行缺口，owner提取不能将二者按“已抽取”核销。本轮仅做静态复核；无测试、编译、性能测量、规则官网复查或Git写操作。
