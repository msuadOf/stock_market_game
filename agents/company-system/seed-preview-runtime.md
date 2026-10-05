# seed 预览与运行接缝

本接缝让新局配置与实际运行共享唯一 seed：App 首次建立预览时读取 crypto，React StrictMode 的重复初始化不重新抽取；交易 E2E 继续固定 seed。新局创建、远程共享市场创建和远程重置均传入已展示的规范 u64 十进制字符串，不在创建调用中再次抽取。

`SeedDraft` 仅保存 UI 草稿的 seed、配置来源与 JSON。默认虚拟预设随合法 seed 输入同步；本人编辑后标记 `custom`，之后 seed 输入保留本人 JSON。明确重新生成才抽取新 seed，并按当前合法配置保留趋势、税务等参数而重新生成初值。熵获取或 JSON 校验失败保留原草稿并显示错误。未完成 seed 文本保留在输入框，创建前严格拒绝，不能静默改成固定 seed。

预设的月、季度、半年、年结算周期选择继续使用同一展示 seed，并按 1／3／6／12 自然月重建对应长度的期初营收和开支基准，不重新抽熵。本人编辑来源不自动切换或覆盖周期；应在 JSON 中明确指定周期和匹配期初基准，或明确重新生成为预设后再选择。两个 App 配置入口复用同一草稿函数，非法输入显错并保留原文。

外层 App 用同一 state 原子安装 setup 与 seed。运行中恢复使用存档 seed；Native 自动恢复优先实际 startupContext setup／seed。恢复安装仅改 AppShell 展示草稿，不改外层创建 state，因此不会因恢复 seed 回写触发宿主重新创建。已取消的异步恢复确认不能覆盖下一局草稿。远程已有市场仅显示其实际 context，不应用本地新局配置；只有 Server 没有市场时，登录页才显示新局预览。

## 简单单元测试证据

- `seed-draft.test.ts`、`company-config-commands.test.ts`、`remote-login-behavior.test.ts`：14 个 case 通过，约 1.2 秒。涵盖完整 u64、custom 不自动覆盖、明确再生成、失败无半份草稿、Remote 创建禁止第二次抽熵、非法 seed 不发送创建请求。
- `startup-recovery.test.ts`、`app-startup-wiring.test.ts`：12 个 case 通过，约 3.7 秒。真实 App 回调检查预览只抽一次、原子 setup／seed 跨重挂、本人编辑保留及熵失败显错。
- `session-host-lifecycle.test.ts` 精确筛选 new／E2E／0 seed／Native actual seed／迟到恢复／创建顺序：6 个 case 通过，约 0.4 秒。
- 年化／四周期契约切换后，重新验证前三批 32 个 case 通过；新增四周期同 seed／匹配金额／custom 保留及真实 App 周期回调共 3 个 case 通过。周期字段增量交由根任务统一独立复核，不另起重复审查。
- 每批通过进程外 10000ms deadline、Node case timeout 10000ms，多文件并发 4。未运行复杂回归。

完整旧 lifecycle 和 save-command 恢复测试依赖正式存档 fixture，目前该 fixture 缺少当前必填 `company_system`，不能作为本轮恢复成功证据；未手补真实 fixture、未削弱断言。TypeScript 定向编译尝试在 10000ms 外部 deadline 内未完成，进程树已终止，未报告编译通过；最终编译和正规 fixture 生成由根任务统一安排。

独立复核结论另见 `seed-preview-runtime-review.md`；未完成复核前不宣称接缝验收完成。
