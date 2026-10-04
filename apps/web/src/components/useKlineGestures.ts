import { useEffect, useRef } from "react";
import type { KlineViewportAction } from "../mobile/market-model.ts";
import { KlineGestures } from "./kline-gestures.ts";

/** 只接管图形区的滚轮和双指；未选中时单指拖动保留页面滚动，选中后拖动更新详情。 */
export function useKlineGestures(onAction: (action: KlineViewportAction) => void, onTap: (x: number, width: number) => void, following: boolean, onDismiss: () => void) {
  const root = useRef<HTMLElement>(null);
  const callbacks = useRef({ onAction, onTap, following, onDismiss });
  callbacks.current = { onAction, onTap, following, onDismiss };
  useEffect(() => {
    const element = root.current;
    if (!element) return;
    const gestures = new KlineGestures();
    let tap: { x: number; y: number; svg: SVGSVGElement; moved: boolean; startX: number; startY: number } | null = null;
    let pinching = false;
    let ignoreClickUntil = 0;
    const svgFor = (target: EventTarget | null) => target instanceof Element ? target.closest<SVGSVGElement>("svg.msd-candle-chart, svg.msd-k-volume, svg.msd-kdj") : null;
    const distance = (touches: TouchList) => Math.hypot(touches[0].clientX - touches[1].clientX, touches[0].clientY - touches[1].clientY);
    const select = (svg: SVGSVGElement, clientX: number) => { const rect = svg.getBoundingClientRect(); callbacks.current.onTap(clientX - rect.left, rect.width); };
    const wheel = (event: WheelEvent) => {
      if (!svgFor(event.target)) return;
      event.preventDefault();
      const delta = event.deltaY || event.deltaX;
      const action = gestures.wheel(delta * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? 400 : 1), event.shiftKey);
      if (action) callbacks.current.onAction(action);
    };
    const click = (event: MouseEvent) => {
      const svg = svgFor(event.target);
      if (svg && performance.now() >= ignoreClickUntil) {
        if (callbacks.current.following) callbacks.current.onDismiss();
        else select(svg, event.clientX);
      }
    };
    const hover = (event: MouseEvent) => {
      const svg = svgFor(event.target);
      if (svg && callbacks.current.following && performance.now() >= ignoreClickUntil) select(svg, event.clientX);
    };
    const start = (event: TouchEvent) => {
      const svg = svgFor(event.target);
      if (!svg) return;
      if (event.touches.length === 1) { tap = { x: event.touches[0].clientX, y: event.touches[0].clientY, svg, moved: false, startX: event.touches[0].clientX, startY: event.touches[0].clientY }; pinching = false; if (callbacks.current.following) event.preventDefault(); }
      else { event.preventDefault(); tap = null; pinching = true; gestures.startPinch(distance(event.touches)); }
    };
    const move = (event: TouchEvent) => {
      if (pinching && event.touches.length >= 2) {
        event.preventDefault();
        const action = gestures.pinch(distance(event.touches));
        if (action) callbacks.current.onAction(action);
      } else if (tap && event.touches.length === 1 && callbacks.current.following) {
        event.preventDefault();
        if (Math.hypot(event.touches[0].clientX - tap.startX, event.touches[0].clientY - tap.startY) > 8) tap.moved = true;
        tap.x = event.touches[0].clientX; tap.y = event.touches[0].clientY;
        select(tap.svg, tap.x);
      } else if (tap && event.touches.length === 1 && Math.hypot(event.touches[0].clientX - tap.startX, event.touches[0].clientY - tap.startY) > 8) tap = null;
    };
    const end = (event: TouchEvent) => {
      ignoreClickUntil = performance.now() + 600;
      if (event.touches.length !== 0) return;
      if (!pinching && tap) {
        if (!tap.moved && callbacks.current.following) callbacks.current.onDismiss();
        else select(tap.svg, tap.x);
      }
      tap = null; pinching = false;
    };
    const cancel = () => { tap = null; pinching = false; ignoreClickUntil = performance.now() + 600; };
    element.addEventListener("wheel", wheel, { passive: false });
    element.addEventListener("click", click);
    element.addEventListener("mousemove", hover);
    element.addEventListener("touchstart", start, { passive: false });
    element.addEventListener("touchmove", move, { passive: false });
    element.addEventListener("touchend", end);
    element.addEventListener("touchcancel", cancel);
    return () => {
      element.removeEventListener("wheel", wheel); element.removeEventListener("click", click); element.removeEventListener("mousemove", hover);
      element.removeEventListener("touchstart", start); element.removeEventListener("touchmove", move);
      element.removeEventListener("touchend", end); element.removeEventListener("touchcancel", cancel);
    };
  }, []);
  return root;
}
