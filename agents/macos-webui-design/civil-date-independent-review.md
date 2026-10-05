# Baseline 权威自然日独立复核

2026-10-05。审查 HEAD 28b7150 后完整 tracked diff 及新增 worker-civil-date.test.ts；未参与实现，未修改产品代码、未提交或推送。

## 三项门禁结论

1. 大 A 语义：本批只透传 ProtocolSession 已有 CivilDate，不根据交易日序、tick 或客户端日期重算，也不改变交易日历/报告披露规则。Worker 与 Tauri 均使用既有 parseIsoDate 严格拒绝缺失及不存在日期，没有默认日期 fallback。旧精确净利 gold 未修改，该未解差额不属于本批。
2. 必要性最小范围：缺失 baseline 日期会使读取日终档后的公司资料仍使用开局日期。补全初始、refresh、restore 三条 baseline 是同一契约的必要链条；desktop actor 补字段与 Web 消费端同步，未引入新日历逻辑或状态 owner。测试 fixture 增加必需字段属于契约更新，原 generation/时序断言保留。
3. 边界与跨层：wasm-worker 的 snapshot 与 civil_date 来自同一个同步任务中的同一 handle；restore 后重新取得替换后的 handle，并在 microtask restart 前发送恢复结果及 baseline。desktop actor 在同一串行命令里读取 snapshot 与日期，无异步推进夹在两次读取间。Tauri 保留 generation/timeline 先写、snapshot/date 解析失败保留旧 baseline 的既有恢复边界；成功安装才推进 query epoch。Worker 无效数据显式失败，未放宽已有 stale/dispose/generation 检查。新增 ownership 断言把旧/新 handle 绑定到不同日期，能发现日期取错会话。

## 独立验证

从 apps/web cwd 运行 worker-civil-date、worker-host、tauri-timeline-state、tauri-host、wasm-worker-ownership 五文件，52/52 通过，1.19 秒。显式 test-concurrency=3，case timeout=10000ms，外部 run-with-deadline 10000ms 进程树 deadline。未运行原生构建或整套浏览器验收，git diff --check 通过。

结论：静态与独立短测门禁通过，暂无有效 finding。主 agent 的新增原生 actor 及真实浏览器横竖屏读档日期验收仍需记录最终结果；不能据此声称旧公司财务 gold 或整个终端目标已完成。

## 最终增量复核

正式 architecture 新增的同 authority baseline 日期契约与本批实现一致。Worker 非法日期 fixture 改为实现 WorkerRequestPort，只修正消息端口类型，继续真实触发相同请求解析；没有削弱 undefined/非法日期拒绝断言。tauri-startup-contract 的字符串检查保留 snapshot 解析并补上 parseIsoDate 精确检查，参数与 where 路径随实现同步合理。新原生测试 rustfmt 未改变行为。无新增有效 finding。

Reviewer 从 apps/web 独立运行 worker-civil-date、tauri-startup-contract、tauri-timeline-state：18/18 通过、192ms，concurrency=3、case timeout=10000ms、外部进程树 deadline=10000ms。git diff --check 通过。主 agent 提供原生 32/32 通过（3.71 秒、10 线程、外部 10000ms）及真实 WASM 浏览器 3/3 通过（22.9 秒、workers=3），覆盖恢复日期横竖屏和移动/平板报告；IAB 既有档 2030-01-02 显示正确。

独立审查门禁通过，完整 Web 短批、production build 与 lint 最终结果由主 agent 继续完成并记录。旧公司净利 gold 未解除，整个终端目标不据此标作完成。
