# 在 exec 前固定共享299秒工作窗及300秒外部进程树截止。
from pathlib import Path
import json, os, sys, time
here = Path(__file__).resolve().parent
slot = json.loads((here / 'slot-handoff.json').read_text())
if slot.get('pendingSlotHandoff') is not False or slot.get('rootAuthorized') is not True:
    raise RuntimeError('root尚未授予本批实际前槽交接，拒绝启动')
anchor = int(time.time() * 1000)
node = '/home/linuxbrew/.linuxbrew/Cellar/node/25.8.2/bin/node'
helper = str(here / 'absolute-supervisor.mjs')
os.execve(node, [node, helper, str(anchor + 300000), sys.executable, str(here / 'run-ram-batch.py'), str(anchor + 297000), str(anchor + 299000), str(anchor + 300000)], os.environ)
