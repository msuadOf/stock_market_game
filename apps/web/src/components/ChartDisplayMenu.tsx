import { Popover } from "@blueprintjs/core";
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { KLINE_MOVING_AVERAGES } from "./kline-moving-averages.ts";
import { CHART_INDICATORS, type ChartIndicator } from "./chart-display-options.ts";
import "./chart-display-menu.css";

type Section = "root" | "averages" | "indicator";
interface Settings {
  selected: readonly number[];
  onToggleAverage: (days: number) => void;
  indicator: ChartIndicator;
  onIndicator: (indicator: ChartIndicator) => void;
  averages?: readonly { readonly days: number; readonly color: string }[];
  onEditAverages?: () => void;
}

/** 菜单只负责入口和焦点，均线与副图状态仍由共用图表持有。 */
export function ChartDisplayMenu(settings: Settings) {
  const [open, setOpen] = useState(false);
  const [section, setSection] = useState<Section>("root");
  const [returnTo, setReturnTo] = useState<Section>("averages");
  const trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open || !trigger.current) return;
    // hidden 页面和滚出视口的锚点都不保留 portal，避免菜单悬在其他页面上。
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => !entry.isIntersecting)) setOpen(false);
    });
    observer.observe(trigger.current);
    return () => observer.disconnect();
  }, [open]);
  function close(restoreFocus: boolean) {
    setOpen(false);
    if (restoreFocus) trigger.current?.focus();
  }
  function enter(next: Section) { setReturnTo(next); setSection(next); }
  return <Popover isOpen={open} onInteraction={next => { setOpen(next); if (next) setSection("root"); }} placement="bottom-start" usePortal autoFocus={false} enforceFocus={false} canEscapeKeyClose={false} popoverClassName="chart-display-popover" content={
    <DisplayMenuContent {...settings} section={section} returnTo={returnTo} onEnter={enter} onBack={() => setSection("root")} onClose={close} />
  }>
    <button ref={trigger} type="button" className="chart-display-trigger" aria-label="图表显示设置" aria-haspopup="menu" aria-expanded={open} onClick={() => { setSection("root"); setOpen(!open); }} onKeyDown={event => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setSection("root"); setOpen(true); }
    }}>显示⌄</button>
  </Popover>;
}

function DisplayMenuContent({ section, returnTo, onEnter, onBack, onClose, ...settings }: Settings & {
  section: Section; returnTo: Section; onEnter: (section: Section) => void; onBack: () => void; onClose: (restoreFocus: boolean) => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const target = section === "root" ? menu.current?.querySelector<HTMLButtonElement>(`[data-section="${returnTo}"]`) : menu.current?.querySelector<HTMLButtonElement>('[role^="menuitem"]');
    target?.focus();
  }, [section, returnTo]);
  function keyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" || (event.key === "ArrowLeft" && section !== "root")) {
      event.preventDefault(); event.stopPropagation();
      if (section === "root") onClose(true); else onBack();
      return;
    }
    if (event.key === "Tab") { onClose(true); return; }
    if (event.key === "ArrowRight" && section === "root") {
      const next = (event.target as HTMLElement).dataset.section;
      if (next === "averages" || next === "indicator") { event.preventDefault(); onEnter(next); }
      return;
    }
    const items = Array.from(menu.current?.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]') ?? []);
    const index = items.indexOf(event.target as HTMLButtonElement);
    let next: number;
    switch (event.key) {
      case "ArrowDown": next = (index + 1) % items.length; break;
      case "ArrowUp": next = (index - 1 + items.length) % items.length; break;
      case "Home": next = 0; break;
      case "End": next = items.length - 1; break;
      default: return;
    }
    event.preventDefault(); items[next]?.focus();
  }
  return <div ref={menu} className="chart-display-menu" role="menu" aria-label={section === "root" ? "图表显示设置" : section === "averages" ? "均线设置" : "副图指标设置"} onKeyDown={keyDown}>
    {section === "root" ? <>{([ ["averages", "均线"], ["indicator", "副图指标"] ] as const).map(([value, label]) => <button type="button" role="menuitem" tabIndex={-1} data-section={value} aria-haspopup="menu" key={value} onClick={() => onEnter(value)}>{label}<span aria-hidden="true">›</span></button>)}</> : <>
      <button type="button" className="chart-display-back" tabIndex={-1} aria-label="返回显示菜单" onClick={onBack}>‹ {section === "averages" ? "均线（可多选）" : "副图指标"}</button>
      {section === "averages" ? <>{(settings.averages === undefined ? KLINE_MOVING_AVERAGES : settings.averages).map(({ days, color }) => <button type="button" role="menuitemcheckbox" aria-checked={settings.selected.includes(days)} aria-label={`MA${days}`} tabIndex={-1} key={days} onClick={() => settings.onToggleAverage(days)}><span style={{ color }}>MA{days}</span><span aria-hidden="true">{settings.selected.includes(days) ? "✓" : ""}</span></button>)}{settings.onEditAverages !== undefined && <button type="button" role="menuitem" tabIndex={-1} onClick={() => { settings.onEditAverages!(); onClose(false); }}>编辑均线周期</button>}</> : CHART_INDICATORS.map(([value, label]) => <button type="button" role="menuitemradio" aria-checked={settings.indicator === value} aria-label={label} tabIndex={-1} key={value} onClick={() => { settings.onIndicator(value); onClose(true); }}>{label}<span aria-hidden="true">{settings.indicator === value ? "✓" : ""}</span></button>)}
    </>}
  </div>;
}
