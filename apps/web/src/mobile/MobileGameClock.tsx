import { formatGameClock } from "./market-model";
import "./MobileGameClock.css";

interface Props {
  day: number;
  tick: number;
  variant: "global" | "detail";
}

/** 移动端常驻游戏时钟；所有顶栏共享同一权威时间表示。 */
export function MobileGameClock({ day, tick, variant }: Props) {
  const clock = formatGameClock(tick);
  return (
    <div className={`mobile-game-clock mobile-game-clock--${variant}`} role="timer" aria-label={`当前游戏时间，第 ${day + 1} 个交易日，${clock}`}>
      <span>第{day + 1}日</span><time dateTime={clock}>{clock}</time>
    </div>
  );
}
