# TAX-K2a 条件Bonus原R权益实施

日期：2026-10-27。唯一writer `/root/sol61_tax_k1_resume_fresh`，独占`.worktree/tax-entitlement-k1`。
K1基线 `b0e53af2b511e1b08a1726e8f7d9f82237a64dd4`；K2 v2设计 `0dd70f66`、
SHA `f7aa21a3fd73bbafd857e280c94bf918f467c3d631b3e30c3ca0c1e3bb331a82`，
独立设计裁决根`tax-entitlement-k2-design-review-2026-10-26.md` SHA
`ea0a80e4fdb725d70f3ffd0bcfe53910c6d88cf7e1647e7edfc1dd214501b43b`限定批准条件K2a/b TDD。

## 本片实际范围

新增`ConditionalBonusTaxFacts`的有序RegisterBonus/RealizeBonus、条件actual_credit scope/数量和
必填nullable tax_acquisition引用。原R lots由真实book prefix重放，day必须为完整R前缀日期；
不复制当前持仓权威、不持久化carrier。typed动作身份分别查重，关联同一个StockBonus所得key。
同文本Cash/Bonus事件可共存，同kind同typed动作异事实拒绝，完全重复幂等且不洗倒序。

actual_credit数量×冻结面值决定所得，税务net数量和所得激活frontier各自独立。
支持条件net0无新lot、net-70/实际credit30、net+30/实际credit100、tax取得p而所得激活q；
只在显式验证来源后分类独立tax净取得，其余未映射CorporateAction继续K1拒绝。
无碎股Q×ratio须精确等于实际credit；不支持分次完整发股/未知碎股所得分配，错误含具体数量。

Cash/Bonus使用同一carrier、同一原R持期/限售/实际gross/事件累计half-up evaluator。
旧Cash事件/payment/累加顺序保留，旧生产`outstanding`及旧新lot Bonus basis未改变。
新入口也在每个legacy collection的真实book frontier验证正确Cash可收余额，防止Bonus或未来收入
掩盖历史过收；实际历史少收保留，不改旧receipt.outstanding，不冒称已补收。

K2b的Collect侧事件、显式Cash/Bonus收缴分额与prepare尚未实施；下一片完成，未宣称K2整体完成。
本片没有收缴副作用、账户安装、SaveSlot/Web变化、真实税务日/physical capture推断或R2撤门。
actual_credit是明确条件输入，不是caller可自授的生产trusted来源；真实接缝仍是设计第5节。
原物理新股参加未来R的税龄必须来自真实tax holding事实，不能以本片历史income carrier冒代。

## 真实TDD

| 轮次 | 实际结果 | 结论 |
| --- | --- | --- |
| k2a-red-01 | 4case全红、exit101 | stub委托K1导致无激活net-70实得0而目标420，正向classified Bonus仍被缺映射拒；fixture准备均成功 |
| k2a-green-01 | 全轻harness82/82绿 | 首4个Bonus行为与原78通过 |
| k2a-boundary-red-01 | Bonus13项中12绿/1红、exit101 | legacy Cash过收500被新Bonus500掩盖，错误在目标拒绝断言 |
| k2a-green-02 | 全91/91绿 | 加入逐legacy frontier Cash校验修复 |
| k2a-green-03 | 全94/94绿、exit0 | 最终16个Bonus+原78；真实binary报告1.08s |

首红测试/stub commit `58ee2623`；共同evaluator/13case边界红冻结 `56de2ae4`。
最终red/green各自独立标签的harness源码/binary保在根`.tmp/tax-entitlement-k1`，不会覆盖历史产物。
每轮before/artifact/run JSON与原始build/run日志已逐字保存在`k2a-evidence/`，manifest密封SHA/大小。
所有运行均在编译绑定13个真实源及5个原rlib依赖后执行，运行前后再次验证源/依赖/binary不变。

最终binary SHA `f61813b4707906b8c81093ed5683483e2ac55ab040ea99d1261580b95558e8f0`，
大小26405600B。最终build2.686s，codegen32，进程树50ms采样峰RSS456212480B/23threads。
build由300000ms外部runBoundedCommand监督；测试整命令由10000ms外部run-with-deadline监督、
真实binary `--test-threads=8 --nocapture`，整命令10秒同时硬约束其中每case。
未运行Cargo、冷target、完整engine回归、ts/lint、Web或生产旅程。
轻harness仅增真实conditional_bonus模块/测试symlink和source SHA条目，没有仿写业务库。

16个Bonus新case覆盖长期免税/短期20+80、不等新lot出售、未激活/晚激活、net0/负/正、
同名typed所得、未来Cash/Bonus R、精确半份额/共同缩股、重复动作及冲突、scope/qty/seq/date拒绝、
strict nullable/unknown字段、book+条件事实恢复后继续、无碎股门/非正面值、实际Cash已收和历史过收。
原78case继续包含K1双端partition/反投影/overflow/退休和旧生产模块行为。

## 冻结门

源码只改cash_dividend_tax.rs、entitlement_units.rs、其测试helper可见性与新增两个conditional子文件；
工作/证据在agents本主题。未改外树或main、未推送。必须由原未实施Sol核完整K2a diff后才能宣称本片完成，
通过后再继续K2b，不能用94绿跳过独审或以当前unused生产入口宣称税bug关闭。


## K2a 初审F1修复

未实施Sol完整初审仅发现F1：先校raw prefix/day再去重，导致合法Reg@0/Realize@1后追加
完全同事实Reg@0被拒。原reversed测试错误地将同一注册重试写成应拒，这是我的oracle错误，
现改为不同新typed身份的倒序负例，仍要求拒绝；没有删除/弱化原非重复倒序合同。

新增跨更后frontier重试Register/Realize并保持完整assessment深等的case，`f304b406` 保存真实红。
修复只将typed身份冲突/完全重复判断移至时间顺序检查之前，保首次事件位置；
原样重试不重新注入carrier/激活，异事实先明确冲突。有效唯一流仍校非递减prefix/date。
顺带删除红测中一个对临时clone的无效赋值，不改变断言；工作TDD计划纠正同一错误说明。

review-f1-red真实1fail/exit101，失败为追加原样重试被InvalidConditionalFact拒，fixture准备成功。
review-f1-green全95/95绿（17个Bonus＋原78），含真实重试正控与不同新动作倒序负控。
最终binary SHA `84c198727834d5c3a9f9a68a6c66164b8180a574e34b2768e16aa9acee244f41`，
整运行wall 0.22532962798140943s，8threads/10000ms外监督；build 2.747407049057074s，
codegen32/300000ms外监督。新标签独立binary、13源+5依赖及运行前后SHA/argv/exit均封原件。
初审原对象05a完整保留，修后须原未实施Sol增量复核，K2b尚未开始业务实现。
