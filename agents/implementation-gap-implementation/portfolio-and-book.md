# 资产与盘口：G46、G49、G67

Desktop卖盘标签按真实价格优先rank生成，再反转显示令卖一邻近买一，少于5档不伪造档位。SSR分别核1/2/5档标签与价格。

持仓展示与Rust Account::cost_price/unrealized_pnl统一：净投入/股数正负对称半偶到每股分，浮盈=(现价−舍入成本)×股数。BigInt仅作中间计算，保留当前Web安全整数边界，不能无损展示的结果显式失败，不擅自解决Q01全局Money范围。200股、净投入±200100分、现价1001分，成本±1000分，浮盈200/400200分；合法负成本不隐藏，费用/T+1语义不改。

parseProtocolSnapshot对所有账户的全部持仓代码核own行情；selector和组件删除缺价默认零。非玩家、零股代码及继承属性均不能绕过引用校验，不重做既有delta校验。

3项新增反例旧码真红。修复后local-amount-render SSR4项通过（878ms），position-valuation与portfolio-selector共4项通过（105ms）；外部timeout10s、Node case10000、concurrency2/4。helper补±0.5/1.5/2.5、非半值、零/负/小数/unsafe股数或价格及结果越界。TypeScript定向编译通过，没有回归或浏览器旅程。

非作者web_gap_review完整diff三门禁无阻断，并提出新增guard边界补测；已补helper与非玩家/零股/继承属性case，并按再次复核发现补真实非半值分数。最终完整diff与记录三门禁全部通过，没有产品阻断；reviewer未另行执行回归。
