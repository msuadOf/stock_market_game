import {
  klineWindow,
  MOBILE_KLINE_DEFAULT_CAPACITY,
  MOBILE_KLINE_ZOOM_LEVELS,
  type KlineViewport,
  type KlineViewportAction,
} from "./market-model.ts";

interface Props {
  total: number;
  viewport: KlineViewport;
  onAction: (action: KlineViewportAction) => void;
}

export function KlineViewportControls({ total, viewport, onAction }: Props) {
  const window = klineWindow(total, viewport.capacity, viewport.offsetFromEnd);
  const atSmallestZoom = viewport.capacity === MOBILE_KLINE_ZOOM_LEVELS.at(-1);
  const atLargestZoom = viewport.capacity === MOBILE_KLINE_ZOOM_LEVELS[0];
  const atDefaultZoom = viewport.capacity === MOBILE_KLINE_DEFAULT_CAPACITY;
  return <div className="msd-chart-tools" aria-label="K线窗口控制">
    <button type="button" aria-label="跳到最早历史" title="跳到最早历史" onClick={() => onAction("earliest")} disabled={window.offsetFromEnd >= window.maxOffset}>«</button>
    <button type="button" aria-label="放大K线" title="放大K线" onClick={() => onAction("zoom-in")} disabled={atSmallestZoom}>＋</button>
    <button type="button" aria-label="缩小K线" title="缩小K线" onClick={() => onAction("zoom-out")} disabled={atLargestZoom}>−</button>
    <button type="button" aria-label="窗口左移" title="查看更早历史" onClick={() => onAction("pan-left")} disabled={window.offsetFromEnd >= window.maxOffset}>‹</button>
    <button type="button" aria-label="窗口右移" title="查看更新历史" onClick={() => onAction("pan-right")} disabled={window.offsetFromEnd === 0}>›</button>
    <button type="button" aria-label="复位K线窗口" title="回到最新并复位缩放" onClick={() => onAction("reset")} disabled={atDefaultZoom && window.offsetFromEnd === 0}>⌗</button>
  </div>;
}
