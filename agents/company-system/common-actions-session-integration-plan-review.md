# 共同股本投资者结算接线计划独立复核

审查对象：`common-actions-session-integration-plan.md` 冻结文本、相关 engine API、ADR-0035、Q14、公司行为设计和当前交接。

审查身份：未参与计划编写的独立审查。

## 结论

通过。首轮提出的五项必须修订均已在冻结文本闭合。计划准确区分了 Simple 汇总账面与投资者真实结算，也没有要求等待 `CompanySimulation` dispatch 才开始 Simple 的共同结算工程。以下逐项记录闭合依据及尚需产品契约决定的事项。

## 必须修订项复核

1. **税款收缴 API：已闭合。** 计划第 6 项将 `CashDividendTaxBook::record_payment` 限定为股东税前 gross 红利到账事实，并注明其 `paid_on == tax_book.settled_on` 等调用条件。第 7 项将实际收税单列为 `collect_due(event_id, day, available_cash)`，准确说明非整分 outstanding 返回 `NeedRoundingEvidence`、不能部分收取或写入收缴 receipt；整分欠税才可按可用整数分部分收取并保留余欠。与现行 API 一致。
2. **成交日权威事实：已闭合。** 第 3、4 项以最终候选内已提交 `Account.position` 前后差作为日终净变化权威事实，与当日已提交 fills 交叉验证；明确 `PersonalTradeConfirmation` 是历史证据而非现成日净变化接口，并只在 DayEnd 最终候选提交消费游标。符合现有成交与事务边界。
3. **股东/开户设定不得擅自强制：已闭合。** 第 1、2、3 项将完整 ownership 输入来源、初始税务 lot 日期/来源、逐 holder profile、NPC/新玩家默认身份及开户必填标为待产品契约；无完整股东事实时不得宣称全体股本/派息闭环，同时允许已知账户成交、gross entitlement 与到账独立推进。没有擅自规定默认 profile 或开户失败条件。
4. **行为偏好 preset：已闭合。** 第 4 项允许按公司可见、可编辑、seed 可复现的虚拟 preset，要求值和来源明示，未批准值不得伪装成经济事实；没有禁止可编辑预设，也没有将预设等同方案批准。
5. **失败付款与权利状态：已闭合。** 第 6 项要求每批对全部未付 holder 提交完整 outcome，逐 holder 允许 Paid/Failed，失败原因进入 book receipt 并保留未付权利；只对真实成功到账的适用账户调用 tax `record_payment`，与账户候选、book settle 原子提交。日期条件和外部 holder 税务边界均有明示。

## 其它准确性复核

- **重复失败历史：已闭合。** 计划记录 `582720b1` 已按 holder 修复失败投影、14 项定向测试通过并经独立复核；批次 C 不再要求重修。
- **自然日日结：已闭合。** 仅已登记 holders 逐自然日推进 Registry；零变更只用于既有 holder，无持股新账户不进入 `changes`。符合 `ShareRegistry::close_day` 连续日期、已登记 holder 与 public market movement 约束。
- **登记与税账：已闭合。** 先以 `ShareRegistry::register` 冻结快照，再把同一快照交给 `CashDividendBook::register`；tax `register_dividend` 仅用于有正 entitlement 且适用的 holder，并明确零持仓不调用。
- **除息市场锚：已闭合。** 第 9 项把真实 Market `last_close`、reference-price/涨跌幅消费字段、登记日次一交易日生效时点列为实际接线前置，要求证明校验消费该值，不停留在计算 DTO；保持历史成交价不变且不模拟成交。
- **A 股及 Simple 语义：通过。** 现稿正确限定官方依据的适用范围，个人上市股票规则不推演其他税务身份；Simple 不以展示现金不足拒绝合规分红，真实投资者到账不伪造公司资金来源，认购不补款、回购不伪造成交。未取得的中国结算操作指南仍作为复杂规则的明确边界，不阻断已确认的整数分纯现金基础。

## 后续契约边界

待产品明确完整 ownership 输入来源、初始虚拟取得事实、profile 配置/开户采集方式及偏好 preset 数值。计划已将这些事项正确表述为未决契约，不把它们偷换成当前产品门禁。税账当前以 `AccountId` 为主体且仅支持 `IndividualPublicMarket`；外部及其它身份在对应实现支持前不得声称已完成税务结算。
