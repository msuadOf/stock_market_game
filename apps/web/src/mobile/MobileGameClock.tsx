import { useTradingTimeline } from "../components/TradingTimelineContext.tsx";
import "./MobileGameClock.css";

interface Props {
  day: number;
  tick: number;
  variant: "global" | "detail";
}

/** 移动端常驻游戏时钟；所有顶栏共享同一权威时间表示。 */
export function MobileGameClock({ day, tick, variant }: Props) {
  const timeline = useTradingTimeline();
  const clock = timeline.clock(tick);
  return (
    <div className={`mobile-game-clock mobile-game-clock--${variant}`} role="timer" title={timeline.displayNote ?? undefined} aria-label={`当前游戏时间，第 ${day + 1} 个交易日，${clock}${timeline.displayNote === null ? "" : `，${timeline.displayNote}`}`}>
      <span>第{day + 1}日{timeline.customized ? "·简化" : ""}</span><time dateTime={clock}>{clock}</time>
    </div>
  );
}
