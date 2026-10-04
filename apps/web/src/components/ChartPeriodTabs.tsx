import { useId, type KeyboardEvent } from "react";
import type { MobileChartPeriod } from "../mobile/mobile-ui-state.ts";

const periods: readonly MobileChartPeriod[] = ["分时", "日K", "周K", "月K"];

/** 两端周期入口共用自动激活的键盘行为，图表组合与信息标签独立。 */
export function ChartPeriodTabs({ period, onChange, panelId, variant }: {
  period: MobileChartPeriod;
  onChange: (period: MobileChartPeriod) => void;
  panelId: string;
  variant: "terminal" | "detail";
}) {
  const id = useId();
  function move(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let next: number;
    switch (event.key) {
      case "ArrowLeft": next = (index + periods.length - 1) % periods.length; break;
      case "ArrowRight": next = (index + 1) % periods.length; break;
      case "Home": next = 0; break;
      case "End": next = periods.length - 1; break;
      default: return;
    }
    event.preventDefault();
    onChange(periods[next]!);
    event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  }
  return <div className={variant === "terminal" ? "chart-tabs" : "msd-period-tabs"} role="tablist" aria-label="图表周期">
    {periods.map((item, index) => <button type="button" key={item} role="tab" id={`${id}-period-${index}`} aria-controls={panelId} aria-selected={period === item} tabIndex={period === item ? 0 : -1} className={`chart-tab${period === item ? " active" : ""}`} onClick={() => onChange(item)} onKeyDown={event => move(event, index)}>{item}</button>)}
    {variant === "detail" && <><button type="button" disabled title="等待引擎提供跨日分钟数据" aria-label="五日（等待跨日分钟数据）">五日</button><button type="button" className="msd-more" disabled title="更多周期尚未提供" aria-label="更多周期（即将开放）">更多⌄</button></>}
  </div>;
}
