# 类型检查修复记录 v1

首次 strict TypeScript job 在实现阶段失败，helper 最终报告 `bounded command exited with 2`，该 job 自身返回 1。同期 lint 退出码 0、Node 11 项退出码 0。编译器原始诊断：

- `player-proposals.ts(64,47)`: `Property 'includes' does not exist on type 'never'`。
- `player-proposals.ts(64,97)`: `Property 'join' does not exist on type 'never'`。
- `player-proposals.ts(80,24)`: `Property 'includes' does not exist on type 'never'`。
- `player-proposals.ts(82,23)`: `Type 'never' must have a '[Symbol.iterator]()' method`。

原因是从输入 union 取 `keyof` 得到字段交集。将 tag 改成明确的六成员联合后，最终 `green-v3` 的 strict TypeScript job 退出码为 0。失败是源码类型错误，不是路径或环境失败。JSON 收据因 ignore 规则未提交，本记录保留同一信息。
