# Q11：本人公告送达、信息检查与分类重估

用户已确认：机构及天天盯盘、活跃交易散户在相关信息公开时获知并重评；低频散户按本人每天一两次、隔几天、每周或每月的关注节奏获知。共享缓存、他人阅读和未公开公司事实均不是本人经历。信息重估与实际交易执行分开。

## 生产接线

`NpcAttentionState` 必填保存 `information_cadence` 与 `next_information_check`。`Immediate` 表示相关公开消息订阅；`Daily` 可配置每天1/2次，双次在自然日09:00/18:00检查；`EveryDays` 支持自然日间隔（包括7天）；`Monthly` 按真实公历推进一个月，短月按该月末日落点。日内普通市场观察仍由原独立注意力随机流决定，不因信息订阅而强制下单。

初始参数是可编辑、确定性的虚拟行为假设，不是A股投资者的统计校准：机构及游资使用 `Immediate`；`Momentum`/`Panic` 散户使用 `Immediate`；`Dormant` 分配7天或自然月；`LongTerm` 按本人账户序号分配1至5天；其他散户分配每日1/2次。该信息节奏不替换交易策略、认知档案或市场注意力概率。没有基本面方法的参与者可以获知材料，但估值仍为 `MethodDisabled`，不补造方法。

现行 Session 发布来自 `DisclosureDispatch::run_day_end` 的公开报告与公告，公开时点固定18:00（`information/publication.rs` 对批准、报告期、发生日及公开相位守卫），故日终事务在正式公布成功后、返回持久化候选前执行本人信息检查。非交易自然日也可进行信息检查。订阅相关股票为本人持仓、关注列表、活动计划及既有信念条目，不把公共曝光候选变为全体NPC已知。低频散户日内本人观察可检查到期材料；若本人当日没有交易观察，日终仍执行其已经到期的独立信息关注检查。此检查不写价格观察、历史行情阅读或真实成交经历。

独立 `PublicLibrary` 的 `correct_and_publish` 目前不存在可直接修改 Session 的公开调用入口；它与 Session 的绑定不得被宣称已实现。当前 Session 内只有上述18:00公开接线，不引入假的宿主更正入口；库中合法公开更正由相关本人下一次检查读取。

## 更正与付款分类

普通已获知报告走 `NewMaterial`；新获知的公开更正保留实际触发报告ID，走 `Correction`，绕过普通λ修订。较新中期报告不能掩盖其年度基数的更正：同范围最新年度被更正后，重新计算本人已经获知的中期补充，不能因中期ID已使用而幂等跳过。未优选范围的单体更正不替换本人已获知且优选的合并口径。真实公开 `PaymentFailure` 新增必填 `PaymentObligationStatus`：

- `UncommittedExpense`：支出因无现金而没有执行，不冒称合同违约，包括当前采购、生产、管理支出、贷款发放、购地与开发支出。
- `ContractualPayable`：已登记赔案的支付失败，确认应付款不代表已超过合同期限；当前赔案没有独立到期参数，不冒称逾期。
- `ContractualOverdue`：已存在合同的实际到期偿付未完成，包括借款本金/利息、银行存款本金/利息，公开后可触发 `CreditDefault`。
- `StatutoryPaymentFailure`：所得税缴纳失败，与信用合同违约分开。

分类来自真实生产 caller，不从 `what` 字符串猜测，不将 `CreditDeterioration` 准备风险冒充真实违约，不改原来已有付款失败事实的公开范围和18:00排期。公司和投资者现金、股份、订单、T+1保持原规则；不补现金。

## 验证与待复核

首两case在实现前定义，新接口未定义时属于编译红，不能冒称已经执行的业务失败。Root的第一次实际执行中，五个 `notices` case有四个通过；更正case真实失败，表现为 `NewMaterial report6` 而非 `Correction report7`，证据 `.tmp/checklist-wave3/session-notices-tests-relevant_correction_is_delivered_as_direct_revaluation_not_ordinary_material.log`。独立复核同样发现较新中期遮蔽年度更正、赔案未到期却标逾期、日终获知缺少因果记录，均已按根因修复，并由 `build5` 刷新编译后重跑通过，未弱化更正断言。

新增 Rust 短case覆盖自然月/双检查时点、相关订阅者隔离与无订单/价格经历、低频本人隔离及存档恢复、公开合同违约分类/提前读取守卫、年度更正重算较新中期、真实休市日日终18:00公开送达与因果零延迟、重估失败整个日终原子回滚、忽略更正时无条目及跨公司材料的明确错误，以及真实保险经营caller只登记应付款。日终获知提交成功后登记本人实际 `Acquisition`，下一次交易诊断来源报告从本人实际 `used_report_ids` 读取，不因材料已读而丢失来源。

Root 的 `build5` 使用 `--jobs 32` 成功编译后，将8个 `notices` case和真实保险经营分类case共9项按 exact 名称并行执行，均通过，单项墙钟0至0.49秒，满足普通测试10秒硬上限。原真实红的更正case最终0.21秒通过，日终失败回滚0.32秒通过，非交易日正式送达0.49秒通过；日志为 `.tmp/checklist-wave3/<完整case名>-final.log`。85项 `ts-rs` 导出实际执行通过（0.33秒），生成 `NpcAttentionState` 和 `NpcInformationCadence` 新契约。未执行完整回归。

Web `npc-attention.test.ts` 两case在10秒外部deadline及10000ms case timeout下已通过（2026-10-05，最新命令墙钟0.41秒）：新关注状态严格保存、拒缺字段与非法参数；付款失败四分类严格保存、拒缺字段。既有 `company-contracts.test.ts` 的3项付款失败定向case同样通过，命令墙钟1.20秒，保留原有金额、active注入、日期及字段拒绝断言。真实引擎fixture仍由 Root 统一重生，不迁移旧档或添加schema版本；该集成收口完成前不宣称三宿主全通过。非作者完整及修复增量静态复核通过，最终复核又读取实际生成类型及9份 Rust 日志，确认每份均实际执行1项并通过，没有零测试假绿。
