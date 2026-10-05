# 共用图表状态与浅色数据面独立复核

2026-10-05，基线 a7a5f09。读取完整 diff、所有新增源码/测试，排除 webui-service.pid。未参与实施，未改产品源码。

## 初审：三项有效发现待处理

1. **P2：暗色盘口平价文字仍被旧样式覆盖。** 新 .msd-order-book .flat 使用 --msd-muted，但 desktop-terminal.css 的 .desktop-terminal .msd-order-book .flat 更高优先级仍读取外壳 --flat。已核对 shared-chart-state-e2e-final.log：20通过1失败，实测 rgb(154,169,187)，预期 rgb(119,119,119)。需要移除或同步旧覆盖，保持白底数据面独立颜色契约。
2. **P2：两套 TSX 测试不被现行 Node 测试执行器支持。** 虽然发现器匹配 .test.tsx，执行器没有 TSX 转译入口。shared-chart-state-short-final.log 明确两个文件 ERR_UNKNOWN_FILE_EXTENSION。应使用可执行的 .ts 测试入口与 Vite fixture JSX，或既有等效方案；不能仅凭更名后的 lint 通过跳过执行。现有断言本身没有被删除，失败在模块加载。
3. **P2：短历史显示窗口与首次右移动作不一致。** MarketKlinePanel 用 klineWindow clamp 存储窗口后显示，changeChartViewport 却将未 clamp 的旧存储值直接交给 reduceKlineViewport。例证券日K361根 earliest 得 offset289，较短周期/历史80根显示 offset8；点击右移先计算289-18再 clamp 为8，屏幕无变化，正确应从显示窗口8右移至0。需要在显式操作时先按当前 total 归一化再 reduce，补长历史→短历史→右移测试。无需周期变化时自动改写存储，避免隐藏实例互相干扰。

## 三项门禁

- 大 A 语义：MA、KDJ/MACD计算、价格/成交量单位及聚合算法未改，单独 chartSettings 属展示状态，不进入游戏存档或 engine。共用 MA/indicator 与按证券窗口符合交互目标；selectedTime 仍局部，切股不会沿用另一证券详情。无新交易规则，无需重新推导制度。
- 必要范围：独立 slice 复用既有 RTK，无新增依赖；保留共用 viewport 模型。浅色数据面拥有自己的文字/深度变量符合实际主题缺陷，未擅自改 MA 色板。测试 Provider 适配必要，但当前 TSX 执行问题必须解决。
- 边界与复杂度：默认窗口使用稳定常量，证券窗口互相隔离，均线/指标支持显式校验；需补短历史操作边界。测试展示了跨横竖屏、切股与返回、详情局部状态、暗色两端可读性；最终运行结果尚不能宣称全通过。

有效发现已逐项发给主 agent，等待修复后再次复核。不认定完整终端目标完成。

## 三项修复再次复核

三项代码/测试 finding 已全部关闭，完整最新源码 diff 未发现新的阻断问题：

- 删除桌面重复盘口变量和 rise/fall/flat 覆盖，颜色统一由共用浅色数据面拥有；不靠继续叠加更高优先级修补。
- 恢复 .ts 测试入口，通过现有 Vite 编译单一 ChartSettingsFixture.tsx，内部 JSX Provider 每次 SSR 使用独立 store。未新增依赖，未修改执行器，既有 SSR 几何/金额/单位断言均保留。手机测试两项既有 children-prop 告警仍在，未压制，不称整份测试文件 lint clean。
- changeChartViewport 先将存储窗口按当前 total 归一化，再 reduce；新短历史首次右移测试明确验证 offset289 经80根历史操作后为0。没有在 render 或隐藏图表更新时派发重置，不会由隐藏实例抢写显示设置。

独立重跑 chart-settings、MarketKlinePanel、mobile-component-render 三套，23项全部通过，约0.808秒，case及进程树 deadline10000ms、并发3；git diff --check通过。核对 shared-chart-state-e2e-verified.log 为21项全部通过、17.3秒，含跨横竖屏/切股状态保持与暗色数据面。

文档检查时 DESIGN.md 155行、UX-CONTRACT.md 118行仍写跨实例使用局部默认值，主 agent 已说明正在同步，已提醒直接替换旧合同而非仅追加互相冲突的新段落。代码审查通过，最后等待该文档漂移同步；不据专项通过宣称全回归或完整终端目标完成。

## 最终文档与门禁结论

DESIGN 和 UX-CONTRACT 的旧跨实例默认值描述已直接替换，canonical owner 表明确 chartSettings 的 MA/indicator 与按证券 viewports；当前会话、刷新不持久化、selectedTime 局部范围及较短历史归一化均与代码一致。浅色数据面 token 归属同步且未冒称完整暗色主题验收。最后文档漂移关闭，三项有效 finding 均已修复复核，本批独立审查门禁正式通过，无未修复有效 finding。

核对 shared-chart-state-production-build.log：生产构建完成，release WASM verified。shared-chart-state-full-e2e.log 为41项中37通过、4失败（56.2秒）；暂停偏好通过，剩余两项公司报告、活动委托与日终存档仍失败。全量 Web 短测缺真实 desktop 生成 acl-manifests.json，2 shards失败；全局5项既有 children-prop 告警继续记录，不将相关生产文件 lint 通过说成所有测试文件 lint clean。git diff --check通过。

允许将本批实现及其真实验证结果提交审阅；此结论不等于全回归通过或完整终端目标完成，截图与工作表由主 agent 如实补记。
