import { ensurePagesIsolation } from "./pages-isolation";
import "./index.css";

const root = document.getElementById("root");
if (!root) throw new Error("游戏启动失败：缺少 root 页面容器。");

async function start(): Promise<void> {
  if (import.meta.env.MODE === "pages") {
    root!.textContent = "正在准备静态网页的多线程运行环境；首次访问将在游戏载入前刷新一次。";
    const ready = await ensurePagesIsolation(import.meta.env.BASE_URL, {
      isolated: window.crossOriginIsolated,
      secure: window.isSecureContext,
      href: window.location.href,
      serviceWorker: navigator.serviceWorker,
      replace: (url) => window.location.replace(url),
      cleanUrl: (url) => window.history.replaceState(window.history.state, "", url),
    });
    if (!ready) return;
  }
  const { renderApp } = await import("./render-app");
  renderApp(root!);
}

void start().catch((error: unknown) => {
  root.setAttribute("role", "alert");
  root.textContent = `游戏启动失败：${String(error)}。未载入或修改游戏存档。请将页面地址、浏览器版本和完整错误反馈给项目维护者。`;
});
