# G59：保险保障期限

保险经营日程创建新赔案须处于coverage_end当日或之前，仍由elapsed与claim_every_days控制发生节奏；期后不再无依据生成新事故。服务释放不修改，保障期内真实发生的未付赔案仍可期后pay_claim，底层record_claim的接受集合不扩张。

新增真实CompanyOperations短Fixture，单组保障2030-01-01至01-03，每日赔案日程，开始日已发生1笔未付；截止01-03共3笔，01-04/05不增加，01-06支付原未付成功。旧码红测期后错误增至5笔，修复后1项通过（0.00s）。构建`--no-run -j16`，执行`RAYON_NUM_THREADS=8 timeout 10s <company_operations-binary> audit_insurance_ --test-threads=8`；无回归、浏览器或长验收。

非作者restore_batch_review独立审查完整代码及测试，确认保障结束日边界、服务释放和期后旧赔案支付均保留，三项代码门禁通过，无阻断发现；绿测与最终台账再次复核通过。
