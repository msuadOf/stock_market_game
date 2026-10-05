import type { ReactNode } from "react";
import type { CoordinateTick, KlineTimeTick } from "./kline-coordinates.ts";

interface Props {
  className: string;
  height: number;
  label: string;
  ticks: readonly CoordinateTick[];
  times: readonly KlineTimeTick[];
  format: (value: number) => string;
  children: ReactNode;
}

/** HTML 坐标不随 SVG 拉伸；三个图形区使用相同的左右坐标留白。 */
export function KlineCoordinatePlot({ className, height, label, ticks, times, format, children }: Props) {
  const labels = ticks.map(({ value, y }) => <span key={value} data-edge={y / height < .2 ? "top" : y / height > .9 ? "bottom" : undefined} style={{ top: `${y / height * 100}%` }}>{format(value)}</span>);
  return <div className={`kline-coordinate-plot ${className}`}>
    <div className="kline-coordinate-axis kline-coordinate-mirror" aria-hidden="true">{labels}</div>
    <div className="kline-plot-canvas">
      <div className="kline-coordinate-grid" aria-hidden="true">
        {ticks.map(({ value, y }) => <span className="kline-grid-horizontal" key={value} style={{ top: `${y / height * 100}%` }} />)}
        {times.map(({ label: time, x }) => <span className="kline-grid-vertical" key={time} style={{ left: `${x / 390 * 100}%` }} />)}
      </div>
      {children}
    </div>
    <div className="kline-coordinate-axis" aria-label={label}>{labels}</div>
  </div>;
}

export function KlineTimeAxis({ times, selected }: { times: readonly KlineTimeTick[]; selected: KlineTimeTick | null }) {
  return <div className="kline-time-row"><div className="kline-time-axis" aria-label="游戏交易日坐标">
    {times.map(({ label, x }, index) => <span key={label} className={index > 0 && index < times.length - 1 && index % 2 === 1 ? "kline-time-secondary" : undefined} style={{ left: `${x / 390 * 100}%`, transform: index === 0 ? "none" : index === times.length - 1 ? "translateX(-100%)" : "translateX(-50%)" }}>{label}</span>)}
    {selected !== null && <span className="kline-selected-time" style={{ left: `${selected.x / 390 * 100}%`, transform: selected.x < 60 ? "none" : selected.x > 330 ? "translateX(-100%)" : "translateX(-50%)" }}>{selected.label}</span>}
  </div></div>;
}
