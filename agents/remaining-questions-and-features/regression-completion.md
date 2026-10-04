# 完整回归修复与通过记录

## 修复范围

用户要求将此前完整回归中的失败全部跑通。本批在现有工作分支修复，再合入 main，
不更改失败即停、每普通 case 十秒或必要长验证五分钟门禁，不放宽断言、增加数字
Money 兼容、改写交易制度或引入日内持久化。

自然日/公开报告夹具不再把合法经营公告误当成日期重复或报告事件；唯一末尾日期
推进、连续 seq、真实公开库内容、休市行情/RNG 不变均严格核验。成交夹具使用
真实自然卖盘或明确本人观察，不要求默认随机策略必然成交。直接初始化持仓同步
登记现行 holding epoch，原费用、预算、股份、T+1、收据与身份守恒检查保留。

代表性公司 fixture 减少重复准备，但真实两整年经营重放及所有原断言保留。
执行器全部竞价/连续、1/2/4线程、Canonical/Reverse/RotateLeft 矩阵拆成独立短 case，
增加原 prototype 状态不变检查；PreOpen 故障注入基于真实收据游标而非硬编码。
Web 修复过时金额错误与异步控制测试契约；Server 认证夹具使用当前分字符串。
独立 scripts 入口在外部监督的 worker 内准备 workspace 临时环境，保持 containment。
各项非作者完整 diff 复核与原红/绿证据见同主题 `regression-*.md`。

## 完整回归结果

正式命令 `node scripts/run-full-regression.mjs` 在源码提交 `e250a7c` 上退出0。
原始日志 `.tmp/main-regression-2026-10-05/full-regression-final.log`；此前两个失败
批次及定位输出同目录保留，不把失败/取消的测试计入最终通过数。

- 构建：128 Cargo jobs，76个预构建测试目标，约57.33秒。
- 执行：约217.45秒；完整构建和执行合计274.78秒。各阶段均受进程外300000ms
  监督，普通 case 单独10000ms deadline；执行前后源码指纹相同。
- Rust 普通测试：2348项全部通过；最多64个 case 并发，各1个 harness thread
  和1个 Rayon worker。清单查询最多8个 binary 并发。
- 必跑长验证：公司经营/披露跨年1项通过，约4.77秒，1个 harness thread、127个
  Rayon worker；不是跳过 ignored 后宣称完整通过。
- workspace doctests：通过，约5.18秒。
- 全部 Web 普通测试：通过，8分片，约2.81秒。
- 全部 scripts：34个文件通过，4个文件 worker，约9.83秒；每文件及每 case
  十秒门禁不变，失败清理未被绕过。

密封 inventory `.tmp/full-regression/artifact-inventory.json` 的源码摘要为
`c66ef7f4c00978c96923e9b7836acaf40a779fb5265e023343891133ed54ee7f`，
inventory 自校验摘要为
`06d1da394990f7da462d0306070d4fc355344dae4b688636b2625618352c2950`。
结果绑定完整源码/必要未跟踪输入与实际产物 SHA，不用相似文件名替代实际构建来源。

## 结果边界

本结论是根测试命令定义的完整回归通过，不等于真实浏览器 E2E、三平台重新发行、
统计/性能最终验收或所有可选 feature 组合通过。未来未定 Q 项、多人账户与云端
产品接线也不因此核销；root runner 的默认 workspace 测试范围与正式配置保持不变。
本记录只改工作文档，不改变上述已验证源码指纹；合并到 main 的代码树须保持相同。
