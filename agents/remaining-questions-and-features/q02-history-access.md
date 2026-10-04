# Q02 历史读取接线

## 完成范围

- Engine 为每个账户维护独立历史读取簿，记录股票代码、读取次数和读取时的市场分钟；历史读取事实与个人价格记忆分离。
- 玩家详情页在切换股票、进入 K 线周期或切换 K 线窗口时显式请求历史；请求结果进入日 K 数据状态。仍以当前实时投影中的 candle 为准，不让较旧响应覆盖已到达的当前 K 线。
- EngineHost、WASM Worker、Tauri、Server 与 Remote 已接通查询接口；三个引擎宿主沿用请求／generation 校验。当前 Server 仍是单玩家会话，三宿主固定绑定 `AccountId(0)`，不是多人账户授权实现。
- 响应使用共享严格 parser 校验请求股票代码、精确字段集合、日 K Money 字符串、成交统计与时间顺序。Server 查询拒绝额外的 `account` 参数。
- `SaveSlot` 必须提供与 snapshot 账户一致的 `history_reads`；旧字段或缺字段存档继续严格拒绝，不添加兼容回填或迁移。

## 语义边界

读取簿记录的是本人显式请求后由 Engine 抽象层实际读取并成功生成响应的访问事实，不代表玩家确实逐根看过、理解或记住全部 K 线；UI 丢弃迟到响应不改写已经发生的访问事实。NPC 的历史读取只在相关历史实际被决策消费时登记。Snapshot、baseline、缓存和预取不产生读取事实。

历史查询当前返回该股票完整可用日 K 历史，复制成本随返回量增长；本批未扩成分页或范围接口。该 API 不改变交易、撮合、资金、股份或 A 股交易时段语义。

## 验证

- Engine `history_reads_survive_civil_day_save_restore_without_mutating_snapshot_or_price_cache`：3 tick 开市日内完成查询、日终、SaveSlot 保存与恢复；检查读取簿保留、Snapshot 和参与者个人价格记忆不因查询改变。root 精确短测通过；构建记录 `.tmp/q-decisions/history-final-build-4.jsonl`，执行记录 `.tmp/q-decisions/engine-history-save.log`。
- Server 私有路由认证综合 case `session_private_http_routes_require_the_matching_bearer_token` 通过，case 内包含历史查询正确凭据请求返回 200 及额外 `account=1` 返回 400 的断言；不是另一个独立命名的历史查询 case。执行记录 `.tmp/q-decisions/server-history-auth.log`，构建记录 `.tmp/q-decisions/history-final-build-3.jsonl`。
- Desktop native no-run 构建通过，75 秒；日志：`.tmp/q-decisions/desktop-build.jsonl`。Server、Engine、native WASM binding 构建也通过；Engine、Server no-run 日志：`.tmp/q-decisions/hosts-build-2.jsonl`。
- 最终 Web 定向批次 13/13 通过：runtime 7项包含真实 hook 状态更新与请求隔离，严格历史响应 parser 1项包含多组正／负控，完整存档 schema 5项。命令使用 10 秒 case timeout 与进程外 deadline supervisor；整批 3.74 秒，不扩大为全 Web 通过。
- TypeScript 项目构建与修改文件的 Oxlint 检查通过。

## 未覆盖

- 未完成浏览器端真实交互／视觉验收，也未完成独立 WASM target 构建与运行；native WASM binding 编译不等于 WASM target 验收。
- 未运行整仓回归，也未取得完整 Web 回归通过结论。并行任务曾误启全 Web 短测批次，旧 fixture 缺少 `history_reads` 等契约错误导致失败；本轮不把这些失败抹去或宣称全绿。真实重生成 fixture 后，定向测试又发现原投影强加机构 policy、机构及价格记忆负例选取的旧假设，均保留严格 parser 与原拒绝断言修正。最终仅定向复测，代表性存档 schema 5/5 通过。
