# 薄复用W1已审subreaper与ECHILD回收；不另建进程清理算法或延长原始deadline。
from pathlib import Path
import json, os, runpy, sys, time

directory = Path(__file__).resolve().parent
anchor, python = int(sys.argv[1]), sys.argv[2]
now_wall_ms, now_mono_ns = time.time_ns() // 1_000_000, time.monotonic_ns()
if not 0 < anchor + 296000 - now_wall_ms <= 296000:
    raise RuntimeError('subreaper必须沿用原始296秒工作窗')
owner = runpy.run_path('/data1/<USER>/workplace/stock_market_game/.worktree/d3-p1-integration-2026-10-28/agents/branch-completion/wasm-w1/combined/owner.py')
# 预留200ms供已清场owner写回执并退出，外层在297秒前确认。
deadline_ns = now_mono_ns + (anchor + 296800 - now_wall_ms) * 1_000_000
execution_ns = now_mono_ns + (anchor + 296000 - now_wall_ms) * 1_000_000
result = owner['supervise']([python, str(directory / 'run-ram-batch.py'), str(anchor + 296000), str(anchor + 299000), str(anchor + 300000)],
    directory, budget_ms=(deadline_ns - time.monotonic_ns()) // 1_000_000,
    deadline_ns=deadline_ns, execution_ns=execution_ns)
result.update(supervisorPid=os.getppid(), completedAtUnixMs=time.time_ns() // 1_000_000)
with (directory / 'subreaper-owner-receipt.json').open('x') as stream:
    json.dump(result, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
if not result['success'] or result['completedAtUnixMs'] >= anchor + 297000:
    sys.exit(1)
