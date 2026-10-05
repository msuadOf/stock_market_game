# Seed 预览与运行接缝独立复核

复核范围：`seed-draft.ts`、启动页、登录页、`App.tsx` 新局设置接线、`useSessionHostLifecycle.ts`、`useSaveCommands.ts` 及所列定向测试；同时检查调用方与 `CompanySystemInput`、公司预设 helper。仅审查，未修改实现。

## 结论

PASS。当前接缝满足本轮 seed 预览和实际运行状态要求；未发现需要阻断合并的 P 级问题。

1. 大 A 语义：本范围将 seed 用于模拟重现与虚拟 Simple 公司预设，不改变沪深 A 股交易规则、单位或真实市场事实。UI 明确说明预设为虚拟简化模型；未发现把预设包装成真实市场统计或影响交易制度的跨层漂移。
2. 必要性与范围：初始预览只抽取一次；新局消费展示 seed；预设重生成显式触发；保存恢复及宿主回报的实际 setup/seed 优先。这些接线是预览与运行一致所必需。`origin` 仅存在于 UI draft，未写入存档/API。范围内变更与需求相符。
3. 边界：seed 使用规范十进制及完整 u64 边界校验；非法输入不能启动新局；preset seed 更新重算预设，custom JSON 不被覆盖；重新生成发生熵错误时保留旧草稿；remote 进入既有市场由 market context 覆盖本地预览，创建新市场直接使用预览 seed/config；恢复来源和宿主实际启动信息不会触发再次抽 seed 的重启循环。

## 验证限制

作者报告的定向短测覆盖 helper、公司配置命令、remote 登录、startup/wiring 与 lifecycle，结果为绿。未独立重跑测试；旧 real-fixture 的缺失 `company_system` 导致相关 restore case 未改造，不能据此声称其通过。宽范围仓库 diff 同时包含其他并行变更，本复核仅对列明的 seed/runtime 接缝作结论。
