# 每公司共用阅读选择独立复核

2026-10-05。完整读取当前 tracked diff 与新增 store/company-reading.test.ts，包括 Redux、ConnectedCompanyPanel、CompanyPanel、展示类型、原 hook 测试改写、新 E2E 与 DESIGN。未参与实现，未修改产品代码或提交。

## 三项门禁

1. 大 A 语义：新增内容是界面阅读位置，不进入游戏存档，不修改公司公开报告、日期、金额和会计公式。精确/缩写仅切换展示，原财务 gold 断言保持。该 UI 状态调整不需要新增交易所制度假设。
2. 必要与最小范围：三项 local state 统一到 readingByCompany，解决同公司横竖屏实例不一致及切公司沿用旧报表/金额选择。报告、报表、金额三个字段按公司分离，patch 不覆盖其他字段，未新增依赖或持久化迁移。CompanyPanel 使用 controlled 回调，状态写入集中在 reducer；store 引用 company-presentation 中的静态枚举，该纯 TS 模块只有类型依赖，没有 React 或 store 回路，当前未造成运行期分层循环。
3. 边界/复杂度：baseline 替换同时清理报告 cache 与 reading，旧 generation 更新先被拒绝；未知报告、非法报表和非布尔金额状态明确报错。effect 仅在 ready/empty 且可见选择不一致时 patch，不在 loading/error 抹掉选择；两个实例使用同一 cache 与 visibleReports，重复 effect 最多提交相同状态，不形成持续互相覆盖。分页追加保留已可见报告 ID，所选 ID 不再属于可见页时仍沿用既有回退契约。没有发现新的有效 finding。

## 测试与诚实性

原 hook fixture 改成真实 reducer 驱动 controlled props，保留 8/7/9 选择断言并新增返回原公司恢复 8；新 store 测试覆盖三字段隔离、patch、重置、旧 generation 与非法值。新增 E2E 验证切股及横竖屏双向修改，没有替换旧财务精确断言。主 agent 区分首次错误 locator 超时与修正 locator 后实际行为失败，前者不算有效 TDD 红证据，记录诚实。

Reviewer 从 apps/web 运行 company-reading、company-report-selection、company-view-model 三文件：实际 9/9 case 通过、514ms，concurrency=3、case timeout=10000ms、外部进程树 deadline=10000ms。没有重复全量 E2E 或构建。主 agent 提供相关 23/23 短测、4/4 真实 WASM E2E（11.6 秒）结果。

结论：独立静态与定向短测门禁通过，无剩余有效 finding。完整 Web 回归及 production build 最终结果由主 agent 补充；既有精确净利 gold 差异与整个终端目标均不据此关闭。

## 存档日期验收同步增量复核

只读检查新 e2e/quick-archive.ts 与两个 suite 的 diff。readQuickArchive 为原实现抽取，仍通过只读事务读取实际 IndexedDB 值并在完成/失败时关闭连接，不写入 fixture 档或替换 WASM 返回。新增 expect.poll 解压真实 gzip 档并同时核对 snapshot.tick=30 与 civil_clock.current_date=2030-01-03，再执行原读档按钮和横竖屏日期断言；原已读档提示及日期断言的默认等待期限未延长。

该同步对本 case 必要：generic 日终提示可能是更早的休市候选，不能证明目标日终档已提交。增加明确存档前提强化了目标来源检查，不是假造成功或弱化 UI 结果。金额和日期数据未重算，使用实际档内的市场 tick 与自然日两个不同概念；gzip/JSON/字段错误会导致验收失败，没有 catch/fallback 静默绕过。没有发现新增有效 finding。

必须保留范围限制：此 case 验证“目标档提交后读档日期正确”，不验证保存提交中立即点击读档的竞态行为。上一轮完整 48 项中的 46 通过、2 失败（原净利 gold 与一次未收到已读档）是真实历史结果；随后重复日期通过不能抹去该失败。本次调整不是已经证明产品不存在竞态，最终完整 48 项重验结果仍待主 agent 记录。未重复运行 E2E，未修改产品实现。
