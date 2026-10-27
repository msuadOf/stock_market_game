# 绿后负对照，只写独立 artifact，不修改生产源；R1 同时破坏已知查询，R2 仅破坏未知查询。
from pathlib import Path
import difflib
source_path = Path(__file__).resolve().parents[3] / 'packages/engine/src/session/exchange_calendar_model.rs'
source = source_path.read_text()
artifacts = Path('/data1/baiyifan/workplace/stock_market_game/.tmp/mixed-calendar-c1a')
for name, replacement in [('unknown-negative', '.values().next()'),
                          ('unknown-negative-r2', '.get(&stock).or_else(|| self.stocks.values().next())')]:
    mutant = source.replace('.get(&stock)', replacement)
    if mutant == source:
        raise RuntimeError('没有命中 StockId 查询，禁止输出无效负对照')
    (artifacts / f'{name}.rs').write_text(mutant)
    patch = ''.join(difflib.unified_diff(source.splitlines(True), mutant.splitlines(True), fromfile='production.rs', tofile=f'{name}.rs'))
    (Path(__file__).parent / f'{name}.patch').write_text(patch)
