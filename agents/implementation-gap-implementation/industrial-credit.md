# 工商授信处：G58、G79

## 实现与接受边界

borrow和available_credit按LoanState关联的真实ContractBook.counterparty求该lender的未偿本金，开局隐式合同与后续还本仍在同一集合；A用满不能占用B的额度。available_credit现为`Result<Option<AccountingAmount>, IndustrialError>`：无额度是`Ok(None)`，有额度是`Ok(Some(...))`，算术/关联错误显式`Err`，不再用`.ok()`吞错。仓库内全部既有调用者显式处理新增Result。

IndustrialBooks serde及完整SaveSlot validator共用validate_credit_state：本金/已提未付利息非负且各自全量可加总，LoanState关联已登记Borrowing合同/贷款人，合同ID对应map key、数据合法，剩余本金不超过合同本金，计提日不早于起息，ACT/365F余数绝对值不超过1,825,000单位。合法零本金、零利息、余数正负半分端点、到期后继续计提及存档编辑仍可接受；不核历史资产来源，不按授信上限倒改已有负债或给公司补钱。

## 短测及独立复核

专用target构建`--no-run -j 16`；binary执行`RAYON_NUM_THREADS=8 timeout 10s <binary> <filter> --test-threads=8`，可独立binary并发运行，不运行回归。

- 多lender/还款与负贷款/不可加总2项新case在旧码真红；修复后连同新增关联/日期/余数表驱动case，industrial_accounting的audit_credit_共3通过（0.01s）。
- owner私有非法大本金fixture验证available_credit返回具体AmountOverflow且状态不变，1通过（0.00s）。
- 完整SaveSlot负本金与总本金溢出拒绝，1通过（2.83s）；溢出fixture同步追加matching合同，断言`amount overflow in add`，不以缺合同制造假阳性。
- 开局隐式债务恢复及tiny还款余数既有case独立过滤，1通过（0.00s）。

非实施者restore_batch_review审查完整diff，确认ACT余数、期后计提和按lender语义正确；指出新增恢复守卫缺逐项边界测试以及溢出fixture应配完整合同。已补表驱动反例/合法端点及matching合同，定向短测通过，最终diff及台账再次复核，三门禁通过，没有新增发现。不把6项短测说成全回归或公司经营闭环已完成。
