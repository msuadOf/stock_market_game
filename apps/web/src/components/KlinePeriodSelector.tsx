import { KLINE_PERIODS, parseKlinePeriod, type ChartPeriod, type KlinePeriod } from "./kline-periods.ts";

interface Props {
  readonly value: ChartPeriod;
  readonly onChange: (period: KlinePeriod) => void;
}

export function KlinePeriodSelector({ value, onChange }: Props) {
  const selected = KLINE_PERIODS.includes(value as KlinePeriod);
  return <details className="kline-period-selector"><summary>更多周期{selected ? `：${value}` : ""}</summary><label>K线周期<select aria-label="K线周期" value={selected ? value : ""} onChange={(event) => onChange(parseKlinePeriod(event.currentTarget.value))}>{!selected && <option value="" disabled>请选择周期</option>}{KLINE_PERIODS.map((period) => <option key={period} value={period}>{period}</option>)}</select></label></details>;
}
