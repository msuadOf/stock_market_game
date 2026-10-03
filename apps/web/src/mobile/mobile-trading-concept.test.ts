import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { createContext, runInContext } from "node:vm";

const html = readFileSync(new URL("../../../../design/ui/mobile/mobile-trading-concept.html", import.meta.url), "utf8");
const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(match => match[1]);

class Element {
  hidden = false;
  value = "";
  textContent = "";
  dataset: Record<string, string> = {};
  classes = new Set<string>();
  attributes = new Map<string, string>();
  children: Element[] = [];
  parent: Element | null = null;
  className = "";
  onclick: (() => void) | null = null;
  oninput: (() => void) | null = null;
  clickCount = 0;
  readonly classList = { toggle: (name: string, enabled: boolean) => enabled ? this.classes.add(name) : this.classes.delete(name) };
  readonly id: string;
  constructor(id = "") { this.id = id; }
  setAttribute(name: string, value: string | number) { this.attributes.set(name, String(value)); }
  click() { this.clickCount += 1; this.onclick?.(); }
  append(...nodes: Element[]) { for (const node of nodes) { node.detach(); node.parent = this; this.children.push(node); } }
  prepend(node: Element) { node.detach(); node.parent = this; this.children.unshift(node); }
  before(node: Element) { assert.ok(this.parent); node.detach(); node.parent = this.parent; this.parent.children.splice(this.parent.children.indexOf(this), 0, node); }
  private detach() { if (this.parent) this.parent.children.splice(this.parent.children.indexOf(this), 1); }
  querySelector(selector: string): Element | null {
    const find = (nodes: Element[]): Element | null => {
      for (const node of nodes) {
        if (selector.startsWith(".") && node.className === selector.slice(1)) return node;
        if (selector === '[data-v="book"]' && node.dataset.v === "book") return node;
        const nested = find(node.children); if (nested) return nested;
      }
      return null;
    };
    return find(this.children);
  }
  querySelectorAll(selector: string): Element[] {
    const key = selector === "[data-v]" ? "v" : "p";
    return this.children.filter(node => node.dataset[key] !== undefined);
  }
}

function fixture() {
  const ids = Object.fromEntries(["m", "time", "day", "book", "detail", "watch", "ttl", "p", "cliprect", "dot", "po"].map(id => [id, new Element(id)]));
  const chartButtons = ["time", "day", "book", "unknown"].map(v => { const b = new Element(); b.dataset.v = v; return b; });
  const navButtons = ["watch", "detail", "book", "watch", "watch"].map(p => { const b = new Element(); b.dataset.p = p; return b; });
  ids.m.append(...chartButtons, ...navButtons);
  ids.ttl.textContent = "华夏科技"; ids.p.value = "68"; ids.day.hidden = ids.book.hidden = ids.watch.hidden = true;
  chartButtons[0].classes.add("on"); navButtons[1].classes.add("on");
  const chart = new Element(); chart.className = "chart";
  const svg = new Element(); svg.append(ids.cliprect, ids.dot); chart.append(svg);
  const bookOverlay = new Element(); bookOverlay.className = "book-overlay"; chart.append(bookOverlay);
  const times = new Element(); times.className = "times"; ids.time.append(chart, times);
  const document = { getElementById: (id: string) => ids[id], createElement: () => new Element(), querySelector: (selector: string) => selector === "#time .chart" ? chart : null };
  const context = createContext({ document });
  runInContext(scripts[0], context);
  return { ids, chartButtons, navButtons, chart, context };
}

test("原型 controller 聚合现有交互引用与方法", () => {
  const f = fixture();
  assert.equal(runInContext("typeof MobileTradingConceptController", f.context), "function", "交互必须聚合到页面 controller");
});

test("图表切换保留未知面板 no-op 与被点击按钮身份", () => {
  const f = fixture();
  for (let i = 0; i < 3; i++) {
    f.chartButtons[i].click();
    assert.deepEqual([f.ids.time.hidden, f.ids.day.hidden, f.ids.book.hidden], [i !== 0, i !== 1, i !== 2]);
    assert.deepEqual(f.chartButtons.map(b => b.classes.has("on")), [i === 0, i === 1, i === 2, false]);
  }
  f.chartButtons[3].click();
  assert.equal(f.ids.book.hidden, false);
  assert.equal(f.chartButtons[2].classes.has("on"), true);
});

test("同目标 watch 导航仅点击按钮选中，book 派发 chart click 并保留原标题", () => {
  const f = fixture();
  for (const index of [0, 3, 4]) {
    f.navButtons[index].click();
    assert.equal(f.ids.detail.hidden, true); assert.equal(f.ids.watch.hidden, false); assert.equal(f.ids.ttl.textContent, "模拟股市");
    assert.equal(f.navButtons.filter(b => b.classes.has("on")).length, 1);
    assert.equal(f.navButtons[index].classes.has("on"), true);
  }
  f.navButtons[2].click();
  assert.equal(f.ids.detail.hidden, false); assert.equal(f.ids.watch.hidden, true);
  assert.equal(f.ids.ttl.textContent, "模拟股市");
  assert.equal(f.chartButtons[2].clickCount, 1);
  assert.equal(f.ids.book.hidden, false);
  f.navButtons[1].click(); assert.equal(f.ids.ttl.textContent, "华夏科技");
});

test("原型初绘、滑块端点与 42/60/80 分段沿用图稿映射，DOM 搬移后引用仍有效", () => {
  const f = fixture();
  assert.equal(f.ids.cliprect.attributes.get("width"), String(3.6 * 68));
  assert.equal(f.ids.po.value, "12:13");
  for (const prefix of ["(() => {\n  const timeView = document.getElementById('time');\n  const chart", "const intradayChart ="]) {
    const script = scripts.find(s => s.trimStart().startsWith(prefix)); assert.ok(script); runInContext(script, f.context);
  }
  assert.equal(f.ids.dot.parent?.parent?.className, "chart-main");
  for (const [value, y, time] of [[20, 94, "10:18"], [41, 94, "11:08"], [42, 132, "11:11"], [59, 132, "11:52"], [60, 151, "11:54"], [79, 151, "12:40"], [80, 184, "12:42"], [100, 184, "13:30"]] as const) {
    f.ids.p.value = String(value); f.ids.p.oninput?.();
    assert.equal(f.ids.cliprect.attributes.get("width"), String(3.6 * value));
    assert.equal(f.ids.dot.attributes.get("cx"), String(3.6 * value));
    assert.equal(f.ids.dot.attributes.get("cy"), String(y));
    assert.equal(f.ids.po.value, time);
  }
});
