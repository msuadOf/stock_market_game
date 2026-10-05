# B03 MA 设置独立复核

## 首轮审查记录

首轮复核发现：`useMovingAverageSettings` 在 `useState` initializer 中直接读取 `localStorage`；损坏 JSON 或读取异常会于首次 render 抛出，绕过设置 UI 的错误提示。该 finding 已发送实现作者；后续版本已改为 effect 内捕获错误，并在 UI 显示错误。

## 最终审查范围

审查 MA 配置与计算、Redux 图表设置、设置表单、`MarketKlinePanel` / `PriceChart` 消费、`ChartDisplayMenu` 编辑入口、`MobileKlineProjection` 价格域和 `PriceChartRuntime` overlay 资源管理，以及对应短测试。忽略同期其他功能及另一周期任务的无关变更。

## 结论

- **大 A 语义：** MA 是图表技术指标，按所选 K 线根数计算，不改变撮合、交易或 NPC 决策语义。实现从原始分报价字符串转 `BigInt` 求滚动和，完整窗口不足时不输出 MA 点；仅绘图值转 `Number`。Rust 指标源显示不支持 MA，未回退到前端。
- **必要性与范围：** 用户批准的默认 MA5/10/20/30/60、可编辑周期、逐线开关和 Desktop/Mobile 共享本机偏好均已接线。周期严格校验为互不重复的正安全整数，存储只接收当前结构。Redux 是图表消费者的唯一状态 owner；localStorage 读取失败设置 `available=false` 并显错，不安装默认偏好；合法保存可恢复。保存失败不会发布新 Redux 设置。图表 overlay 按 period 复用 series，移除隐藏或删除的线。
- **前次 finding：** 已闭环。新增 UI 测试覆盖损坏 JSON、`getItem` 抛错、不安装默认设置、显错及之后保存合法配置恢复。
- **短测试：** 通过 10 秒外部 deadline 和每 case 10 秒上限，配置、MA、UI、Redux、共享 K 线相关测试共 24 项通过，耗时约 1.6 秒。未执行完整回归或 Rust 测试。

## Runtime finding 复核与关闭

前一轮交叉测试发现旧 `PriceChartRuntime.updateMovingAverages` 用无 `rawPrices` fixture 与精确报价契约冲突。实现方已删除无生产 caller 的旧 runtime 方法及旧 series map；原有多选测试和全部断言保留，改为使用 `rawPrices` 精确输入和唯一 `updateMovingAverageOverlays` 路径。复核确认源码不再包含旧接口或第二个 runtime MA owner。

## 最终结论

两项 finding 均已闭环，未发现遗留问题。最终专项命令通过 10 秒外部 deadline 和每 case 10 秒上限，配置、计算、设置 UI、Redux、共享 K 线及 runtime 测试共 36 项通过，耗时约 1.5 秒。未执行完整回归、Cargo 或全量 TypeScript 检查。

## 最终空状态 hunk 复核

实现最后将 `<MovingAverageSettings {...movingAverages} />` 加入 `MarketKlinePanel` 的空 K 线 return。此处 hook 已先运行并捕获坏存储状态，但早退会遮住表单与错误提示，也会让用户无法保存有效设置来修复偏好；将设置表单放在“等待游戏生成首个交易日 K 线…”提示下方是必要且最小的修复。该 hunk 不创建假 K 线、不改变图表数据或交易语义。最终独立复核通过。

随后新增的 leaf hunk 移除了私有 `ReactReduxContext._currentValue` 读写：测试直接调用当前已加载版本的 `Provider` 生成 element，再核验其 `type` 是同一 `ReactReduxContext.Provider`、value 非空且携带真实 store；hook harness 只响应相同 context 对象，并通过 `React.ContextType` 标注。该方式仍以 dispatcher 驱动 hook，不是完整 DOM render，但 Provider 与 context 身份来自实际 `react-redux` 模块，mock 边界清楚；没有私有 Context 字段、`any` 或忽略诊断，也未改生产行为。四项设置 UI/hook 测试通过 10 秒外部 deadline 和每 case 10 秒上限，4/4 通过，约 0.6 秒。MA 六文件短测仍为 36/36 通过（约 1.6 秒）；全量类型检查由实现方另行处理，本审查不宣称其已通过。
