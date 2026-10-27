# Desktop 首次实际构建未通过

root 交唯一 Cargo 槽后，按已冻结计划 `company-catalog-desktop-2026-10-27/build-plan-v3.json`（SHA `7a11556290041fab20d018273f21c519d575bfc88a3d68d3b40d0c87a15410ed`）运行 session `31834`。目标为既有 `.tmp/company-catalog-bc/target`；`-j8`，外部300000ms、cleanup1000ms、60秒无进展停止、可用磁盘低于20GiB停止，没有放宽阈值。

实际193.525秒后退出1，原因是磁盘 guard：193.174秒采样的可用空间19.9796GiB。日志、386个CPU样本、失败target及build-result均原样保留在根 `agents/branch-completion/company-catalog-desktop-2026-10-27/`；不重启同批，不覆旧失败。依赖阶段实际有8个rustc进程，engine阶段有多线程，不声称所有时段满核。

外层监督器另报告root退出后进程组身份无法确认，不能将这一消息隐藏成清理成功；实际wait后补核/proc的已知root1017146、cargo1017173、rustc1072098及其ppid/pgid/sid均没有存活进程，Cargo/rustc扫描为空，再将唯一槽交还root。磁盘仍持续不足，需要先由root处理资源，不能机械重跑。

desktop-ready/source.sha256的15项源码构建前后均通过。此轮没有得到Desktop测试binary，四个必要case尚未运行，不能宣称Desktop或三宿主通过；更不是业务TDD红灯。Server的真实三个case绿色不替代该门。
