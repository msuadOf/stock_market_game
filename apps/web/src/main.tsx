import "./index.css";

const root = document.getElementById("root");
if (!root) throw new Error("游戏启动失败：缺少 root 页面容器。");

async function start(): Promise<void> {
  // 批 9 A10：Pages 跨源隔离引导（Service Worker 注 COOP/COEP 头）随
  // wasm 多核退役删除——串行 wasm 不需要 SharedArrayBuffer。
  const { renderApp } = await import("./render-app");
  renderApp(root!);
}

void start().catch((error: unknown) => {
  root.setAttribute("role", "alert");
  root.textContent = `游戏启动失败：${String(error)}。未载入或修改游戏存档。请将页面地址、浏览器版本和完整错误反馈给项目维护者。`;
});
