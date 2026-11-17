import { ensurePagesIsolation } from "./pages-isolation";
import "./index.css";

const root = document.getElementById("root");
if (!root) throw new Error("游戏启动失败：缺少 root 页面容器。");

async function start(): Promise<void> {
  // 独立e2e前端产物才提供真实窗口合同桥；默认production/dev页面不分发此入口。
  if (import.meta.env.MODE === "e2e" && import.meta.env.VITE_SESSION_AUTHORITY_CONTRACT === "1" && "__TAURI_INTERNALS__" in globalThis) {
    const { installTauriAuthorityContractBridge } = await import("./host/tauri-authority-contract.ts");
    await installTauriAuthorityContractBridge();
    root!.textContent = "Desktop独立验证入口已就绪；会话由真实tauri-host产品合同驱动。";
    return;
  }
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
