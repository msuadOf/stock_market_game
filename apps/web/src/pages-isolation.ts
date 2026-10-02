type IsolationContext = {
  isolated: boolean;
  secure: boolean;
  href: string;
  serviceWorker: ServiceWorkerContainer | undefined;
  replace(url: string): void;
  cleanUrl(url: string): void;
};

const reloadMarker = "__stock_pages_isolation";

export async function ensurePagesIsolation(base: string, context: IsolationContext, timeoutMs = 8000): Promise<boolean> {
  const page = new URL(context.href);
  if (context.isolated) {
    if (page.searchParams.has(reloadMarker)) {
      page.searchParams.delete(reloadMarker);
      context.cleanUrl(page.href);
    }
    return true;
  }
  if (!context.secure) throw new Error("Pages 多线程需要安全上下文；请使用 HTTPS 或浏览器认可的回环地址。");
  const serviceWorker = context.serviceWorker;
  if (!serviceWorker) throw new Error("浏览器不支持 Service Worker；请使用支持该能力的浏览器并检查隐私设置。");
  if (page.searchParams.has(reloadMarker)) throw new Error("Pages 跨源隔离未生效；已停止自动刷新，请检查浏览器的 Service Worker / SharedArrayBuffer 支持。");
  if (base !== base.trim() || !/^\/(?:[A-Za-z0-9_.-]+\/)*$/.test(base) || base.split("/").some((part) => part === "." || part === "..")) {
    throw new Error(`Pages 资源路径不合法：${base}`);
  }
  await new Promise<void>((resolve, reject) => {
    const finish = (error?: Error) => {
      clearTimeout(timer);
      serviceWorker.removeEventListener("controllerchange", controlled);
      if (error) reject(error);
      else resolve();
    };
    const controlled = () => { if (serviceWorker.controller) finish(); };
    const timer = setTimeout(() => finish(new Error("Pages Service Worker 启动超时；请检查浏览器权限和网络后手动刷新。")), timeoutMs);
    serviceWorker.addEventListener("controllerchange", controlled);
    serviceWorker.register(new URL("pages-isolation.js", new URL(base, page)).href, { scope: base, updateViaCache: "none" })
      .then(controlled, (error: unknown) => finish(new Error(`Pages Service Worker 注册失败：${String(error)}`)));
  });
  page.searchParams.set(reloadMarker, "1");
  context.replace(page.href);
  return false;
}
