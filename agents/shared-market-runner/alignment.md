# 三端对齐范围：权威存档、重同步与实际验收

- 存档：成功日结产生同一个不可变 checkpoint，三个宿主的权威都只做非阻塞入队。Browser 独立 Worker 写现有 IndexedDB 快槽，Native Server/Tauri 共用原生 SQLite 队列。归档事务按世代、writer 租约、日结游标校验，只有提交回执表示已持久化。用户授权的文件副本可由前端发起，不再是主快槽。保持旧档读取，不预先移除或批量重写旧档。日 K 按需查库暂缓。
- 重同步：冻结旧流 → 取新切面 → 让发送端确认切面的精确游标 → 安装基线 → 恢复发包。过期只重取不安装，有界重试；不跨会话世代/连接复用旧游标。Server 新 WS 控制消息留旧入口兼容，重接观察者时新通道序号从零起，以连接内唯一 capture_id 区分；Tauri/Browser 用已有同代观察流的绝对 ordinal。运输差异不决定何时界面对新基线生效。
- 验收：同一 fixture 不发 ACK 直到确认实际日终存档，独立读盘、重开和旧写入交错；共同重同步轨迹涵盖失效基线及晚到旧增量。三端同负载区分权威吞吐、可见吞吐、发布开销和控制延迟，Linux Desktop 需真实 GUI。

## 当前验收记录（2026-11-15，Dot 云工作树）

- Native Server：SQLite 归档 5/5、跨日协议 5/5、真实 WS 13/13（独立性能探针跳过）。新增 WS 检查拒绝过期切面、拒绝提前放行，精确 Fence 接受后仍等 Release。
- Browser：用当前候选 Rust WASM 生成测试 bindings，验证实际 shared memory，再由真实 Chromium 验证两份 UI ACK 均扣留且 IndexedDB 写事务被临时占用时，权威仍跨周末进入下一交易日，界面停在 tick 2，权威到 tick 67。放开数据库而不放回 UI ACK 后实际读到周日 seq 1097 的快槽，含两根周五日 K、无历史分钟，重开产品仓库读到同一 seq。测试结束恢复此前生成包。正式产物构建未验收，不以候选测试替代发布构建。
- TS 相关回归 234 项通过；Desktop 含测试的 Rust 编译检查通过。图形基础设施验证到真实 WebKit JavaScript；完整桌面产品尚未启动，三端同负载性能尚未测齐，不能宣称性能相当。预检/作业日志位于独立构建日志区。
- 已知边界：Browser 首次 UI 启动仍先装配观察 Worker，本轮验证的是启动后即使无 ACK 也能独立推进与落盘；授权文件镜像为额外观察者功能，不影响快速槽或日结工作委托查询。未推送；按需查库明确暂缓。

## 正式制品补验（2026-11-15）

- Web 正式制品：锁定 Nightly 的 release WASM 与 wasm-pack 元数据已生成；构建总期限到点后，独立核对实际 Rust 源闭包和制品哈希，继续执行原有的 WASM 校验、TypeScript、Vite 与 dist 校验。sourceSha256 为 1a735ebd2cceb36d9f222cce9eb9f7cc98ff69987f068f3f6c6fe1c57d95b33d。
- Chromium 153 的正式 Web 独立版实际启动，1× 与 MAX 能切换。扣留两个真实 ACK 时前端停在 tick 19，权威在采样窗口仍推进；放行 ACK 后画面恢复。固定种子的停收回归中，真实 IndexedDB 被占用时市场跨周末，界面停在 tick 2，取回权威基线为 tick 76。放开存储、不恢复收包时读取归档序号 1097，重新打开仍是同一归档。两只股票各落一根周五有成交的日 K，历史分钟为零。
- Server + Chromium 153：真实同源 HTTP/WS 加载同批正式前端，游客登录和共享市场创建成功。界面保持 tick 2 时权威跨周末到 tick 65，SQLite 已提交日终档（seq 1097、两根周五成交日 K、零历史分钟）。主动断开真实 WS 后恢复到 tick 75 并继续收包；拉取连接分别验证旧切面拒绝、未 Fence 拒绝提前 Release、持有期间不发及成功 Release。共享市场隐藏本地快速读档，控制者仍可明确选择文件导入。
- Server 标准 ThinLTO 链接超出云工作机本批 300 秒上限；实际功能验收使用优化 release，仅最终 Server 可执行文件关闭 LTO。该结果不作为正式加速比或三端性能数据。
- Linux Desktop：编译真正的 Tauri 优化可执行文件，嵌入同批正式前端，在 Xvfb 上启动实际 WebKitGTK 窗口，并通过受控原生 IPC 验收。观察器停在 tick 2 时，权威跨周末到 tick 63；旧分钟数为零，周五日 K 成交量 871500。读到 SQLite 快槽 seq 1097，退出后重新只读打开数据库并通过 SQLite quick_check。发布器拒绝旧游标，持有时权威继续，接受切面 66 后恢复的首个 ordinal 为 67。为在云机检查真实 WebView 启用了 Inspector，最终 Desktop 可执行文件关闭 LTO，不作为性能对比制品。
