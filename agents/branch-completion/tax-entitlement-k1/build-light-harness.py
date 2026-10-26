"""直接编译仓库真实税账模块；须由获准的进程树 deadline supervisor 启动。"""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys

if len(sys.argv) != 2 or not sys.argv[1].replace('-', '').replace('_', '').isalnum():
    raise SystemExit('需要唯一的字母数字证据标签；不得覆盖已存在的证据')
label = sys.argv[1]
repo = Path(__file__).resolve().parents[3]
output = Path('/data1/baiyifan/workplace/stock_market_game/.tmp/tax-entitlement-k1')
output.mkdir(parents=True, exist_ok=True)
if (output / (label + '-before.json')).exists():
    raise SystemExit('该证据标签已经存在，请使用新标签保留历史记录')
if os.statvfs('/data1').f_bavail * os.statvfs('/data1').f_frsize < 20 * 1024**3:
    raise SystemExit('磁盘可用低于20GiB，停止本片编译')

def source(path):
    return (repo / path).read_text()

def derived_type(text, name):
    end = text.index(';', text.index('pub struct ' + name)) + 1
    start = text.rfind('#[derive(', 0, end)
    return text[start:end]

account = source('packages/engine/src/account.rs')
order = source('packages/engine/src/orderbook.rs')
spec = source('packages/engine/src/company/spec.rs')
company_display = spec[spec.index('impl std::fmt::Display for CompanyId'):spec.index('/// 公司元数据')]
helpers = order[order.index('pub(crate) mod js_safe_u64'):order.index('/// orderbook 操作失败')]
# rustc 的显式 #[path] 会改变子 module 搜索根；只用 symlink 保持真实源码逐字引用。
module_links = {
    'company/share_registry/mod.rs': 'packages/engine/src/company/share_registry.rs',
    'company/share_registry/tests.rs': 'packages/engine/src/company/share_registry/tests.rs',
    'company/cash_dividend_tax/mod.rs': 'packages/engine/src/company/cash_dividend_tax.rs',
    'company/cash_dividend_tax/tests.rs': 'packages/engine/src/company/cash_dividend_tax/tests.rs',
    'company/cash_dividend_tax/entitlement_units/mod.rs': 'packages/engine/src/company/cash_dividend_tax/entitlement_units.rs',
    'company/cash_dividend_tax/entitlement_units/tests.rs': 'packages/engine/src/company/cash_dividend_tax/entitlement_units/tests.rs',
}
for relative, original in module_links.items():
    link = output / relative
    link.parent.mkdir(parents=True, exist_ok=True)
    if link.is_symlink():
        if link.resolve() != (repo / original).resolve():
            raise RuntimeError('module symlink 指向其他源码，停止覆盖：' + str(link))
    elif link.exists():
        raise RuntimeError('module 路径已有非 symlink 文件：' + str(link))
    else:
        link.symlink_to(repo / original)
code = '\n'.join([
    '#![allow(dead_code)]',
    'mod account { ' + derived_type(account, 'StockCode') + ' }',
    'mod orderbook { ' + helpers + derived_type(order, 'AccountId') + ' }',
    '#[path = ' + json.dumps(str(repo/'packages/engine/src/calendar/date.rs')) + '] mod calendar;',
    '#[path = ' + json.dumps(str(repo/'packages/engine/src/money.rs')) + '] mod money;',
    'pub use account::StockCode; pub use orderbook::AccountId; pub use money::{Money, MoneyError};',
    'mod company { ' + derived_type(spec, 'CompanyId') + company_display,
    '#[path = ' + json.dumps(str(output/'company/share_registry/mod.rs')) + '] pub mod share_registry;',
    '#[path = ' + json.dumps(str(output/'company/cash_dividend_tax/mod.rs')) + '] pub mod cash_dividend_tax;',
    '}',
])
harness = output / 'light-harness.rs'
harness.write_text(code)
deps = Path('/data1/baiyifan/workplace/stock_market_game/.tmp/company-read-views-target/debug/deps')
args = ['rustc', '--edition=2021', '--test', str(harness), '-C', 'debuginfo=0', '-C', 'codegen-units=32', '-L', 'dependency=' + str(deps), '-o', str(output/'light-harness')]
dependencies = []
for name in ['serde', 'serde_json', 'thiserror', 'ts_rs', 'num_bigint']:
    matches = list(deps.glob('lib' + name + '-*.rlib'))
    if len(matches) != 1:
        raise RuntimeError(f'依赖{name}需唯一实际artifact，得到{matches}')
    args += ['--extern', name + '=' + str(matches[0])]
    dependencies.append({'path': str(matches[0]), 'sha256': hashlib.sha256(matches[0].read_bytes()).hexdigest()})
paths = [repo/p for p in [
    'packages/engine/src/account.rs','packages/engine/src/orderbook.rs','packages/engine/src/company/spec.rs',
    'packages/engine/src/calendar/date.rs','packages/engine/src/money.rs','packages/engine/src/company/share_registry.rs',
    'packages/engine/src/company/share_registry/tests.rs','packages/engine/src/company/cash_dividend_tax.rs',
    'packages/engine/src/company/cash_dividend_tax/tests.rs','packages/engine/src/company/cash_dividend_tax/entitlement_units.rs',
    'packages/engine/src/company/cash_dividend_tax/entitlement_units/tests.rs',
]]
records = [{'path': str(p), 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]
receipt = {'label':label,'harness_sha256':hashlib.sha256(harness.read_bytes()).hexdigest(),'args':args,'files':records,'dependencies':dependencies}
(output/(label+'-before.json')).write_text(json.dumps(receipt, ensure_ascii=False, indent=2)+'\n')
build_env = os.environ.copy()
build_env['TS_RS_EXPORT_DIR'] = str(output / 'bindings')
subprocess.run(args, check=True, env=build_env)
if any(hashlib.sha256(Path(r['path']).read_bytes()).hexdigest() != r['sha256'] for r in records + dependencies):
    raise RuntimeError('编译中源码或依赖漂移，产物无效')
receipt['binary_sha256'] = hashlib.sha256((output/'light-harness').read_bytes()).hexdigest()
receipt['binary_bytes'] = (output/'light-harness').stat().st_size
(output/(label+'-artifact.json')).write_text(json.dumps(receipt, ensure_ascii=False, indent=2)+'\n')
