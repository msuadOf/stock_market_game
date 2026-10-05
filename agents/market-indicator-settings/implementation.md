# B03 可编辑 MA 显示偏好

用户已回答：周期列表可编辑，默认 MA5/10/20/30/60。各线具有独立显示开关；Desktop 与 Mobile 共用显示偏好，不写入市场存档，也不改变 NPC 认识或执行策略。玩家继续默认 frontend 计算，选择 Rust 时目前 MA 明确不支持，不静默回退。日／周／月／季／年及分钟 K 周期已由用户另行批准，由独立周期任务负责，不与本批 MA 设置混写。

周期必须是正安全整数且不可重复，显示标志必须显式 boolean；存储只接受当前结构，不引入迁移或兼容。计算以原始分字符串转 BigInt 求滚动和，完整窗口不足时不出 MA 点，仅末端图形坐标转换为 Number。保存失败保留原已生效偏好并在 UI 显错，读取损坏也不能默认补齐。

首轮五项短单测使用未实现 stub：四项实际失败，一项非法输入拒绝负控通过；不把负控通过当实现证明。实现后五项全部通过。随后加入真实设置表单、双消费者偏好同步／失败保留、Runtime复用句柄及移除旧线短测；共14项通过，整命令外部10000ms监督，Node concurrency=4，每case10000ms，实际0.62秒。不执行复杂回归。

按用户要求合入最新 `feat/ui-design` 后，两端生产K线统一使用 `MarketKlinePanel`，不恢复旧 Mobile 独立 KlinePanel。`chartSettings` Redux 保持唯一周期／显示 owner：`averagePeriods` 是可编辑列表，`selectedAverages` 记录逐线开关。`useMovingAverageSettings` 只负责 localStorage 严格读取／保存和同页／跨页刷新，不另设一份均线权威状态；已有显示菜单保留开关并加入编辑入口。保存失败不 dispatch 新设置，两端保留原有效显示。

均线对完整已聚合K线历史求值后才裁剪可见窗口，避免用可见窗口从头计算。共用 `klineMovingAverage` 使用原始分字符串的 BigInt 滚动和，末端坐标才近似；默认60周期不足60根没有伪造的“短窗口MA60”，文本显示“—”。自定义period参与真实 SVG、图例与详情，绘图价格域包含可见开启的MA点。选择 Rust 时 MA 明确不支持、不回退；原 KDJ／MACD 数据源守卫同时保留。

前次独立复核指出首渲染读取损坏 JSON 会绕过表单错误。现改为 effect 受检读取：错误设置 `available=false` 并显式展示，原 Redux 有效偏好保持、不把默认值安装为损坏内容的替代；明确保存一组合法设置后恢复可用。新增真实 Redux store／Provider context 的双消费者、坏 JSON、getItem 异常、保存恢复四项短测。新增可编辑 Redux 行为先取得 stub 真失败，再实现；最终共享消费者／自定义实际SVG／设置桥／严格校验／Redux 共22项短测通过，外部及case10000ms、并发4、实际1.49秒。

最终复核还指出旧 `PriceChartRuntime.updateMovingAverages` 没有生产 caller、仅旧测试调用，且与新 overlay map 重复。已移除死亡接口及旧资源 owner；原多选／完整均价／不改K线及窗口／不足周期清空／分时释放断言保留，迁到唯一 overlay 接口并提供对应精确原始分值。六套定向短测36/36通过，外部及case10000ms、并发4、实际1.55秒；非作者独立重复相同短测36/36通过约1.5秒，全部有效发现关闭，见 `review.md`。这不包含全仓回归或尚在收敛的跨模块TypeScript检查。

合入前本批曾有一次 TypeScript 检查实际8.12秒通过；该结果不外推合并后全仓。合并后的多层源码由 root 统一检查，当前本批不自行运行 Cargo、完整回归或恢复旧UI。最终 TypeScript 与独立复核证据仍需收口。
