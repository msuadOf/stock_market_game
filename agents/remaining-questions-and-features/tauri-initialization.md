# Tauri 初始化失败的资源回收

## 范围与契约

本项处理实现审计的 Q24，仅修改 `createTauriHost` 的初始化失败路径与行为单测。
`engine-event`、`engine-failure` listener 注册与会话初始化属于同一资源获取范围。
第二个 listener 注册失败须释放已取得的第一个 listener；取得有效 `create_session`
返回的 session ID 后，任何 baseline、capability 的调用或解析失败均须释放两个
listener 并等待本人 `stop_session` 确认。没有取得有效 session ID 时不得猜测 ID
或停止其他会话。

清理前使本 Host 失效，清理项独立并行执行；一项失败不会阻断其余清理。
成功清理后的初始化错误保留 `cause`；清理也失败时使用 `AggregateError` 保留原错误、
所有清理错误和含操作名称的可见消息。没有新增格式版本、旧协议兼容或交易规则。

## TDD 与短验证

先添加 `tauri-initialization.test.ts`，再修改实现。初次运行包含等待停止确认的用例时，
旧代码未调用 `stop_session`，被外部 10000ms deadline 终止，不能据此声称具体行为通过。
随后使用 `--test-isolation=none` 与名称选择运行四项可立即终止的用例，结果为 1 通过、
3 失败：第二 listener 失败未进入初始化错误包装与清理；baseline 失败未停止会话；
首个清理失败替换原错误且阻断其他清理。

实现后以外部 10000ms deadline、case timeout 10000ms 验证新增 6 项与原 Host
15 项，共 21 项全部通过，wall-clock 小于 1 秒。单个进程中顺序运行用例是因为
Tauri mock 修改进程级 `window`；TypeScript 检查与测试进程同时执行以利用多核，
检查无错误输出且约 7 秒结束。

额外运行 `tauri-startup-contract.test.ts` 时 26 项有 25 通过，隐藏页面的源码
字符串检查失败，涉及本项未修改的 `useSessionHostLifecycle.ts`；已通知负责主任务的
Agent 同步该层。未执行完整回归或桌面原生启动验证。

非作者 `review_remaining_engineering` 已完整审查 diff、新增测试及原生 stop_session
归属/确认链，A 股语义、必要性与跨层边界三项门禁通过；独立运行新增6项短测全部通过。
未执行完整回归，不把局部回收边界扩大为全部原生故障已有验收。
