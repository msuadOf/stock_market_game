# 本人拒单通知 merge 回退修复

## 范围与真实身份依据

- 仅修改 `apps/web/src/host/protocol/effects.ts`、`reduce.ts`、`protocol-effects.test.ts`、`protocol-reducer.test.ts` 四文件。
- 回退根因是旧 `event.IntentRejected.account !== 0`：当前 AccountId已经是规范 u64 字符串，且 Remote玩家并不固定0，因而合法本人拒单被静默排除。
- 已核 Server `public_baseline_for`／`snapshot_for`／TickBatch RuntimeDelta／CivilRefresh均按认证成员投影本人账户；缺席主体的accounts为空。本地WASM／Native仅提供可信Player0入口，没有公开join成员入口。
- effects输入新增明确 `selfAccountId: AccountId | null`，缺省为null，不从event自己声称的account猜本人，也不补0。复用当前AccountId严格parser，保留u64MAX、不转Number、数字或非规范字符串显式拒绝。
- reducer在同generation和cursor／retry校验后，从**当前已安装scoped权威基线**唯一账户key派生本人。空账户表示无经济成员；多账户表示当前没有唯一scoped本人，两者均为无owner，不选择0或任意第一个账户。未来多玩家本地主机若要求本人操作，需要明确身份传输契约，不能拿多账户原始投影当本人基线。
- `SettlementError`仍显式通知；本地NPC业务拒单保留原事实而不冒充本人通知，Remote他人的金融事实仍由Server投影为PrivateEventOmitted。
- 不改变A股数量、价格笼子、费用、T+1或撮合；只是恢复已授权本人操作反馈。

## TDD 与验证

- 先将测试current AccountId迁为字符串并加入本人非0／MAX、other0不得冒充本人、空／多账户不猜0、系统结算错误、同代retry不重放、新baseline generation／旧代拒绝、严格身份表示case。
- 实现前真实短红：7项中5 fail、2 pass，0.25秒；保留 `.tmp/protocol-effects-owner-red.log`。
- 第一轮实现导入旧函数名canonicalU64而当前导出名为accountId导致模块导入失败，已修为当前既有alias；未将该失败计为业务绿色。
- 本组7/7绿色后邻近22case中21绿色，只有reversed facts测试用空基线却断言两本人notice。经root明确授权，只补合法本人0经济账户基线，保留原两个notice、反序与状态一致断言，没有放宽生产默认值。
- 最终4个test文件同时执行，Node `--test-concurrency=4`，仓库进程树10000ms外部deadline：**29/29 passed、0 failed，0.313秒**，日志 `.tmp/protocol-effects-owner-final-green.log`。
- 再次显式增加Node `--test-timeout=10000` 配置每case硬限，并保持4线程与整命令10000ms进程树监督：**29/29 passed，0.299秒**，日志 `.tmp/protocol-effects-owner-final-timeout-green.log`。不以整命令上限冒充已配置case timeout。
- TypeScript集中检查及非作者完整diff复核由root安排；本作者没有运行Cargo、操作index或提交，也没有修改其他UI消费者。
- 非作者完整四文件及真实调用链复核已通过，见同目录 `rejection-effects-review.md`；复核者独立执行同四suite29/29绿色、0跳过、0.291秒，保持case／整命令双10000ms和并发4。复核确认身份读取更新前同代权威基线，不由新包或event反向决定。TypeScript最终集中结果仍由root登记。
