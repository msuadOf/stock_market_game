# 在 exec 前固定296秒工作、297秒树退出、299秒发布及300秒绝对截止。
from pathlib import Path
import hashlib, json, os, sys, time
here = Path(__file__).resolve().parent
slot = json.loads((here / 'slot-handoff.json').read_text())
if slot.get('pendingSlotHandoff') is not False or slot.get('rootAuthorized') is not True:
    raise RuntimeError('root尚未授予本批实际前槽交接，拒绝启动')
for filename, key in [('plan-template.json', 'expectedPlanSha256'), ('source-freeze.json', 'expectedSourceFreezeSha256')]:
    actual = hashlib.sha256((here / filename).read_bytes()).hexdigest()
    if slot.get(key) != actual:
        raise RuntimeError('最终slot没有绑定独立复核后的实际输入：' + filename)
anchor = int(time.time() * 1000)
node = '/data1/<USER>/workplace/stock_market_game/.tmp/node24-env/node'
helper = str(here / 'absolute-supervisor.mjs')
os.execve(node, [node, helper, str(anchor + 300000), node, str(here / 'cleanup-supervisor.mjs'), str(anchor), sys.executable], os.environ)
