# 个股报价与成交明细独立复核

## 范围与结论

相对HEAD e29d5cf审查本批完整diff及新增文件：MarketQuotePanel、MarketTradeTape、marketQuoteFacts、共享CSS、桌面接线、移动摘要/资金/明细迁移、测试与DESIGN/UX。reviewer未实施产品代码。当前源码及短测复核通过，未发现阻断性实现finding；缓存展开/收起和移动真实浏览器几何验证仍待主agent补齐，不能据此宣称完整目标完成。

## A 股语义与依据

- 本批不改变交易制度，无需新增交易所规则解释；沿用仓库权威Snapshot/TradeEvent及既有100股/手契约。价格和成交额保持精确分字符串格式化，不从图表number反算；零股250股仍显示2.5手。
- marketQuoteFacts从权威activeDailyCandle读取全天volume/tradeStats，非近期成交缓存累加。存在成交但tradeStats缺失时显式null/不可用；零成交OHLC占位显示--，不会冒充已形成开高低。缺rawPrices显式报错，符合原移动摘要约束。
- TradeTape按code筛选，取成交自身tick（缺失显式说明），沿用store最新在前顺序。默认七笔/展开当前缓存均注明有限缓存，不冒充全天完整逐笔；trade.qty仍为股，统一格式化为手。

## 复用、状态与必要性

- 桌面三个标签是独立信息视图，ARIA关系完整，方向/Home/End键切换和焦点同步。标签状态保留，TradeTape按code key重置展开状态；hidden面板不参与普通焦点导航。
- 两端直接复用明细renderer和事实提取函数，资金页未再建立独立统计算法。状态限于tab/expanded，无engine或持久化改动，符合本次复用需求，复杂度合理。
- CSS对移动窄栏限定字号和表格间距，父msd-ticks固定高度且可滚动，内部缓存列表也可滚动；需真实320/390宽度检查展开按钮可达、内部滚动及价格列，源码检查不替代浏览器证据。

## 独立验证与剩余验收

在apps/web运行market-quote-facts、mobile-component-render、local-amount-render：23/23通过，退出码0，约501ms；case timeout与进程树deadline均10000ms、concurrency=3。git diff --check通过。第一次同一shell搜索使用了根目录相对路径而cwd为apps/web，rg未找到；随后已从正确目录重新核对store排序及E2E，测试本身正常执行。

主agent报告桌面专项14项E2E通过。当前已存在的测试验证零成交、精确分、小数手、缺统计、证券过滤、空缓存以及tab键盘/切股；仍需完成真实缓存>7笔的展开/收起和切股重置验证，以及移动窄栏实际检查。上批完整回归公司/暂停偏好/交易验收失败限制继续保留，不将本批专项通过描述为全回归通过。

## 最终补验复核

- 最终MarketTradeTape保持精确数据与筛选顺序，展开入口移至sticky标题并保留aria-expanded/controls及准确缓存数量；手机隐藏可见单位后仍保留完整th accessible name。缺失成交时间可换行，未缩写或伪造实际价格/数量。
- 新测试锁定先筛选证券再截七笔，9笔本证券与异证券混合时只展示正确7行，并验证展开数量与价格元/量手的accessible name。
- reviewer独立运行相同三个套件：24/24通过，退出码0，约580ms；case与进程树deadline均10000ms、concurrency=3，git diff --check通过。
- reviewer查看quote-detail-mobile.jpg，确认手机右侧明细列和标题收起入口显示，滚动布局未覆盖分时主图。主agent实际两端点击验证600610的38笔缓存可展开38行/收起7行、切股返回恢复false/7行；390/320窄栏scrollWidth与clientWidth分别相同86/64。动态交互与几何数值由主agent取得，本reviewer未重复操作浏览器。
- 本批源码、短测与补验记录最终独立复核通过，无未解决实现finding。范围限报价摘要/有限缓存显示与共享renderer，不改engine或交易制度。
- 验证限制：完整回归仍在运行；此前desktop acl-manifests.json缺失及全局5项children-prop问题继续保留，不能把24短测及专项E2E通过描述为完整回归或全部目标通过。最终完整回归结果由主agent另行记录。

## 键盘可达性最终增量

- 三个tabpanel增加tabIndex=0及focus-visible，hidden非活动面板仍不进入Tab顺序；当前tab保留roving tabindex，Tab可进入其纯数据内容。新增E2E直接验证行情标签→Tab→行情面板焦点，未改业务数据或选择状态。该增量范围必要，静态复核无新finding，git diff --check通过。
- 已核对terminal-fidelity-plan本批记录，明确24短测/14专项E2E与完整26项21通过5失败的区别，且没有把未做baseline对照的失败一概归因为既有。全量unit的acl-manifests.json缺失及全局5项children-prop限制保留。新增焦点专项E2E结果由主agent完成后记录，本reviewer没有将运行中结果宣称通过。
- 最终结论保持：本批独立复核通过，可以在焦点专项验证结果确认后按授权提交；不代表完整产品目标或全回归完成。提交时排除本机webui-service.pid临时文件。
