# 自选与证券搜索独立复核

日期：2026-10-05。复核者未参与产品实现。范围为当前完整 working diff，包含新增源码与测试，排除服务 PID；未修改产品代码、未提交。

## 初审结论

初审曾发现一项 P2 顺序与 Enter 目标不一致问题；现已修复并经再次复核关闭，最终结论见文末。

### P2：过滤后恢复全部导致 AG Grid 行序漂移

MarketGrid 将过滤结果交给既有 MarketGridRowSynchronizer，而 diffMarketRows 只产生 add/update/remove。AG Grid 未提供 addIndex 时把新增行追加尾部（核对已安装 ag-grid-community 的 executeAdd / sanitizeAddIndex）。原始列表 [600101,002156,…] 查询 002156 后清空，旧的 002156 行保留在首位，600101 等恢复行追加；SecurityListControls 的 Enter 仍按 visibleCodes 第一项打开 600101。屏幕第一项与搜索 Enter 目标不一致，且无主动排序也发生顺序变化。

应补实际 AG Grid 搜索→清空、范围收窄→恢复的行序与 Enter 首项一致测试，并修复无显式排序时的顺序恢复；保留用户主动列排序。此问题不涉及 engine，但违反共用名单的导航语义。

## 三项门禁

1. **大 A 语义与依据**：本批没有改变撮合、费用、T+1、价格/数量单位或存档。持仓范围基于真实 position.qty > 0，自选不会冒充持仓；保留未知当前局代码但不伪造可交易证券。只改展示偏好，无新增交易制度，使用现有领域契约足够，不需新增交易所规则推导。
2. **必要性与最小范围**：独立 WatchlistPreferences、App 单一 useSecurityBrowser owner、共用筛选与入口符合需求，未增加依赖或修改 engine；选择仍属于 MarketRuntime。查询不会自行选股或改写委托，显式 Enter/点击继续调用已有选股路径。手机返回标题及 fixture 更新有直接需求关系。
3. **边界、跨层与复杂度**：存储校验、先保存后发布状态、读取失败禁用开关均合理；读取失败不覆盖磁盘，写入失败保留上次有效状态。空态区分准确；没有匹配、单条、当前证券被移出过滤集时不虚构相邻证券。方向键目标与焦点同步，无新增重复 selectedCode。主要遗漏是上述 AG Grid 恢复顺序集成测试。建议同时补权限/损坏修复后重试成功的正向恢复测试；当前恢复实现静态可行，现有 E2E 仅覆盖重复失败。

## 独立验证

独立执行：

```sh
cd apps/web
node ../../scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=3 src/config/watchlist-preferences.test.ts src/app/security-browser-model.test.ts src/mobile/mobile-ui-state.test.ts src/app/local-amount-render.test.ts
```

20 项全部通过，约 0.85 秒；case 与整命令 deadline 均为 10000ms，并发为 3。覆盖自选存储校验、筛选交集/顺序、键盘模型、空/单/缺失证券相邻边界、移动导航状态，以及既有金额/手数/持仓语义 SSR。

未独立重复全量验收；主 agent 的本批 E2E 和构建结果仍须结合最终日志记录。此前全回归公司报告、暂停偏好、活动委托失败，unit 缺生成 acl manifest 及全局五项既有 lint 警告，不得据专项通过宣称全回归通过或完整终端目标完成。

## 最终修复复核

再次检查完整最新 diff 后，初审 P2 已关闭，未发现新的阻断问题。本批独立复核门禁通过；不代表完整终端目标或全部回归通过。

- MarketGridRowSynchronizer 在成员或代码顺序改变时先 flush 旧事务，再设置带稳定 ID 的有序 rowData；正常报价更新仍只发布变化行。submittedRows 在 API 成功后才更新，flush 或 setGridOption 抛错不会前移提交状态或静默吞错。dispose 解绑后不再提交。该扩展直接针对新增筛选产生的结构变化，未泛化为不必要的新状态层。
- 搜索 Enter 在桌面读取 AG Grid 实际首行，因此尊重主动列排序；手机读取 mobileRowData 首行，不受隐藏桌面排序影响。RowApiModule 属于既有 AG Grid 的必要 API 模块，没有新增包。
- 新顺序测试覆盖过滤恢复、旧事务先完成、普通报价仍单行 update；旧测试的删除/新增预期调整为新的结构替换契约，保留迟绑定、重新 attach、不重复提交、异常不前移等断言。E2E 按真实屏幕 top 排序校验行序，并核对 Enter 与主动价格排序，不再假设 DOM 子节点顺序就是显示顺序。
- useSecurityBrowser 将初始 load 与用户 reload 区分；重试成功明确通知并撤除错误/禁用状态，读取失败不触碰存储。保存成功后清除此前失败提示，不冒称失败仍是当前状态。正向恢复 E2E 已补，保留原损坏数据与持续失败测试。
- UX-CONTRACT 已同步结构替换、两端首行和恢复提示规则。其余语义与范围结论不变。

独立追加验证：前述命令增加 src/components/market-grid-row-synchronizer.test.ts，24 项全部通过，Node 报告约 4.64 秒（整命令约 5.09 秒），10000ms case/进程树 deadline、并发 3。git diff --check 通过。

核对主 agent 的 security-browser-e2e.log 为 26 项通过（22.0 秒）；此日志形成于新增隐藏桌面排序→手机版首行用例之前，该新增用例主 agent 单独报告已通过。本次查看 security-browser-full-e2e-final.log 时完整验收仍在运行，不提前记录最终结果。此前完整日志为 31 通过、5 失败，失败仍为两项公司公开报告、暂停偏好及两项活动委托/日终流程。保持全回归、unit 生成文件及 lint 限制的诚实报告。

## 价格格式与最终验收补记

最后增量仅删除手机列表低价时多补的末尾 0，并新增真实 SSR 断言。centsToYuanText 已返回两位元价，855 cents 应显示 8.55；fixture 昨收 900 cents 对应 -5%，没有以不合法主板价格变动构造本用例。两端沿用相同精确转换，不影响比较、排序、交易或资产。该增量无 finding，前述通过结论保持。

独立再次运行 local-amount-render、watchlist-preferences、security-browser-model、market-grid-row-synchronizer 四套，17项全部通过，约0.79秒，10000ms case/整命令 deadline、并发3。

核对 security-browser-full-e2e-final.log：39项、3 workers，34通过、5失败，耗时1.3分钟；失败为两项公司报告、暂停偏好、活动委托与日终存档。该批完整 E2E 在最后价格显示修复之前完成，价格修复由新增 SSR 覆盖，不将先前 E2E 冒称价格修复后的全量验收。最终生产构建日志为 security-browser-production-build-final.log，复核时由主 agent 执行。主 agent 报告 full unit 仍缺 acl-manifests，10核机器8测试进程 CPU 62%–88%，完整 lint 仍5项旧 children-prop 警告；本 reviewer 未重新采集 CPU，不把此数值记为独立测量。

复核时 terminal-fidelity-plan.md 尚未追加本批事实段，已提醒主 agent 补入最新事实与上述时间界限；不据本批通过宣称原完整目标完成。
