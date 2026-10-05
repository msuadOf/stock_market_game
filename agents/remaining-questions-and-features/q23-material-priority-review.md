# Q23：本人材料种类优先级独立复核

## 范围与结论

本次由未实施排序改动的 agent 复核 `strategy/fundamental/update.rs` 的 `report_kind_rank`、四元 `own_known_report_priority`、年报选择缓存 key 类型及四个 `material_priority_tests`，并阅读该文件完整上下文、`session/decision_chain/roots.rs`、`retail_analysis.rs`、`session/notices.rs` 的实际消费者，以及 ADR-0016 与 Q23 用户回答。既有 `Monthly` 中期接线不属于本次排序独立提交范围。

静态复核通过，未发现本范围阻断项。本 agent 按任务限制没有运行 Cargo、网络研究或修改生产源码；随后亲自读取 root 提供的 `host28` 红测及 `host29` 四项指定绿测日志，确认下述有限测试门禁通过，不将其外推为完整回归。

## 领域与依据

- 排序是用户确认的游戏认识策略，不冒称 A 股投资者法定估值要求：先报告期间，再既有 `Consolidated` 优先，之后同期间同范围的 `Annual` ＞ `HalfYear` ＞ `Quarter` ＞ `Monthly`，最后同种版本序。
- `preferred_own_report` 仅遍历 `NpcObservationContext::acquired_reports`，随后通过本人上下文读取报告；不会把公共库里本人尚未获知的年报纳入选择。公司过滤、同 scope 年度基准与归母数据处理保持原有守卫，排序没有混合单体与合并金额。
- 本批不改变撮合、股份、资金、税务或披露排期，不引入 schema 版本、旧档兼容及迁移。年度和半年度窗口分别使用合法 December／June 的真实报表生成入口，不用同期间不存在的年报／半年报组合假装法定报告。

## 必要性与边界

- 新 rank 放在版本序之前，直接修复高版本月报挤掉同期间同范围年报的缺口；缓存类型仅随 helper 返回值同步，未引入第二套排序。
- 机构普通观察、散户观察、即时公告送达及更正比较均消费同一个 helper 或 `preferred_own_report`，没有仅修改某一种 NPC 的漂移。
- 四个测试覆盖全部种类 rank、December／June 同期排序、高版本月报不能盖过年报、同种更正版本、较新期间／原有合并范围优先，以及真实公开库中未获知年报的排除与随后本人获知后的选择。
- 非年度更正材料只用于合法报告元数据排序，测试不声称完整公开库更正链已验收；未知材料用真实 `ClosingEngine`／`PublicLibrary`／本人 acquisition 链验证。该区分与记录一致。
- 本次测试证据仅覆盖报告种类排序，不包含另批 `Monthly` 中期两个分支或新增 pending baseline 刷新测试；这些不能借本复核记录宣称通过。

## 实际短测证据

- `.tmp/checklist-wave4/host28-same_period_same_scope_prefers_annual_half_year_quarter_monthly_before_version.log` 显示退出码101，真实失败为合法 December 同期间排序 `december.windows(2)` 断言（`update.rs:608`），不是编译失败或非法 fixture 引起的失败。同批未知材料和期间／scope 两项守卫各执行1个 case 并通过。
- `.tmp/checklist-wave4/host29-four_report_kind_ranks_follow_confirmed_order.log`、`host29-same_period_same_scope_prefers_annual_half_year_quarter_monthly_before_version.log`、`host29-report_kind_rank_does_not_override_newer_period_or_consolidated_scope.log`、`host29-preferred_material_excludes_unacquired_annual_even_when_public.log` 均实际执行1个指定 case，四项合计4通过、0失败；日志中的单项执行时间为0.00–0.01秒，没有零 case 假绿。
- root 登记执行配置为4个 case 并行、`RAYON_NUM_THREADS=8`、每命令进程外10000ms deadline，批次0.11秒。本 agent 直接核验的是上述测试结果日志；未另行重跑编译或测试。静态完整范围复核与指定红绿证据共同支持本次最小排序提交门禁通过，不代表整批月报、全宿主或完整回归已完成。
