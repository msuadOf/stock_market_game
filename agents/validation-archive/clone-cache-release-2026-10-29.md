# 2026-10-29 Calendar34 空间阻断后的旧 clone cache 清理

状态：root因新的真实preflight阻断授权固定19同名旧ELF；已通过未实施Sol独审并实际删除，sync及command均exit0。Calendar34可用21,253,611,520B，低原20GiB门221,224,960B，正式图未launch且唯一槽HOLD；前轮“当时余量够”的事实不能保证后续磁盘余额。本次目标释放固定旧副本，避免每300MB重复维护，不扩大盘点。

唯一目录：`/data1/baiyifan/workplace/stock_market_game/.tmp/main-mid-regression-2026-10-26/source/.tmp/build-cache/full-regression/debug/deps`。固定名称严格来自上一批已删除19ELF的原plan，不追加名称；重新检查19件全部同UID普通nlink1、ELF头正确，Linux FIEMAP显示shared=0/无unknown extent，exclusive extent长度合 **8,826,462,208B**，与allocated相同。它们是此前未删的另一路径clone cache；不是恢复旧已删除raw，也不把同名当字节相等证明。

| 固定ELF名称（上列目录） | allocated/exclusive bytes |
|---|---:|
| `engine-ec537d051c5d4a67` | 597999616 |
| `stock_market_game_lib-0ef0880103169caf` | 555388928 |
| `api_contract-ac113fe2018da2ba` | 499490816 |
| `ws-dde321042f9ed124` | 497422336 |
| `server-a5d88607731fa10f` | 481845248 |
| `shared_market_rest-fff12d45ce2fa318` | 480350208 |
| `pause_security-c5d2c520b871fb5f` | 472522752 |
| `identities-f2fbae170f81386f` | 471990272 |
| `actor-3984bf2fb3c53cdb` | 471953408 |
| `healthz-65c482952f603812` | 470892544 |
| `deployment_options-6f593fe731093a51` | 469716992 |
| `protocol_updates-5cf6efb50fa9ae75` | 458907648 |
| `shared_market_restart-c1e29dc9a4c183e1` | 458293248 |
| `producer_guard_isolated-6abfc04b91732992` | 456056832 |
| `session-f11c58478eb89c71` | 405962752 |
| `save_contract-bacf95536b2ec239` | 397045760 |
| `publisher-27b3c51411880931` | 394158080 |
| `company_operations-e87e2a3e89092601` | 394141696 |
| `identity_fixture-756b936d2505b209` | 392323072 |

## 旧证据与来源边界

对应上一批原plan `cache-stability-2026-10-29-plan.json`、实际原19删除收件 `cache-stability-2026-10-29-receipt.json` 和Markdown全部保留。上一批allocated8,788,860,928B而当时df仅增794,148,864B的事实不回写，原逐件source Git SHA/run/binary SHA未可靠取得的限制也保留。本次只read ELF头/stat/FIEMAP，不重hash大件、不重跑测试；不声称clone字节等于原件或历史green。`.d`/fingerprint/源码/原小manifest和日志继续保留。

## 保护与执行

同目录1751非选项stat冻结，全部rlib/rmeta/so/d/依赖cache保留，source本体及node/toolchain/runner helpers不碰；sharedRAMtarget和当前14native+seals/7newpkg/5rawWASM+seals不读写/hash/copy。独立复核前的可读本用户/proc检查 112 进程无19选件引用；权限拒绝PID及可读cmdline按plan记录，不绕过也不冒称全可读。Calendar34唯一槽HOLD且无Cargo，直接执行使用RAMtarget；其确认后才删。

8worker并行unlink，共享外部300秒deadline，普通check/sync各10秒。删除前再次核原stat及FIEMAP unshared，删后sync再核实际df，目标余量5GiB以上但物理净增只报告实际观测，不预先承诺8GB。无论结果如何不新增选择；具体shared/受控/不匹配项保留上报root。正式命令、退出、清场、真实空间与删除日期将追加；删除后原件无法再做字节复核或原ELF直接执行。用户已授权Markdown代替不用raw，不新归档/备份。

本批不修改产品、规则、契约、测试断言或金额/股份单位，A股交易语义保持。

Calendar34直接闭包小依据：`/data1/baiyifan/workplace/stock_market_game/.worktree/calendar-final-gates-2026-10-29/agents/branch-completion/mixed-calendar-c2c/final-gates-e2-source-34/plan-template.json` 与同目录 `runner-freeze.json`（actualInvocation/9helpers）；`preflight-resource-blocked-01.json`保实际HOLD。owner确认只取本批fresh:false Cargo JSON产出的RAM unit，无clone ELF/control/fallback。

## 实际执行与交权（2026-10-29）

原未实施gpt-6.1-sol high逐件核19 stat/FIEMAP、1751非选stat、可读/proc、Calendar34精确runner直接闭包后APPROVED。正式命令为外层`timeout --kill-after=2s 298s python - <固定19件stat/FIEMAP复核、8worker unlink、sync、后核脚本>`；内部`sync -f /data1`独立10秒deadline。执行前19件extent flags均仅0/LAST；session83778亲WAIT chunk16396d **exit0**，sync **exit0**，共享wall约6.554秒，未跑Cargo/测试。19原件已于2026-10-29实际删除，无法再做raw字节复核或原ELF直接执行。

同目录1751非选stat、HEAD/index全保持；source本体/依赖cache及所有小证据继续保留；当前RAMtarget/native/WASM/pkg/各seal/helpers不读写/hash/copy。真实收件`clone-cache-release-2026-10-29-receipt.json`。

/data1实际available从 **22,993,100,800B** 到 **31,818,678,272B**，实测净增 **8,825,577,472B**；原20GiB门以上余量 **10,343,841,792B（约9.63GiB）**，超过本次5GiB余量目标。分列原allocated/exclusive **8,826,462,208B**，不把全盘差值机械等同文件统计。已直接将真实退出、清场和门过收件交Calendar34自行preflight并launch，不等新的root票；本次固定19件处理后停止，没有扩展选择或新增归档/备份。前四批exit1/1/0/0、上一稳定批仅净增794,148,864B的原收件和限制全部保留，不回改历史结论。
