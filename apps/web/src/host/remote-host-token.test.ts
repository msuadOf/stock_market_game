import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer, type ViteDevServer } from "vite";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { remoteTestContext } from "./remote-test-context.ts";

type RemoteModule = typeof import("./remote-host.ts");
const servers: ViteDevServer[] = [];
let production: RemoteModule;
let development: RemoteModule;

before(async () => {
  [production, development] = await Promise.all([false, true].map(async (dev) => {
    const vite = await createServer({
      configFile: false, appType: "custom", mode: dev ? "development" : "production",
      server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true },
      define: {
        "import.meta.env.DEV": JSON.stringify(dev),
        "import.meta.env.VITE_REMOTE_TOKEN": JSON.stringify("compiled-legacy-token"),
      },
    });
    servers.push(vite);
    return await vite.ssrLoadModule("/src/host/remote-host.ts") as RemoteModule;
  }));
});

after(async () => { await Promise.all(servers.map((server) => server.close())); });

test("生产与开发均拒绝缺少真实身份或 market context，不回退 legacy/session token", async () => {
  for (const module of [production, development]) {
    for (const options of [{}, { token: "" }, { context: remoteTestContext() }, { token: "identity-token" }]) {
      let requests = 0;
      const fetchFn = (async () => { requests += 1; return Response.json({ session_id: "legacy", session_token: "legacy-token" }); }) as typeof fetch;
      await assert.rejects(module.createRemoteHost(DEFAULT_SETUP, 1n, { ...options, fetchFn }), /token|身份|context|上下文/);
      assert.equal(requests, 0, "身份/context 校验失败不得创建市场或发送其他请求");
    }
  }
});

test("真实身份同时授权HTTP和WS，订阅与离开均不改变共享市场", async () => {
  for (const module of [production, development]) {
    const requests: { url: URL; init?: RequestInit }[] = [];
    let socketUrl = "";
    let protocols: string[] = [];
    let closed = 0;
    const token = "explicit-identity-token";
    const host = await module.createRemoteHost(DEFAULT_SETUP, 1n, {
      baseUrl: "https://new-server.example/game", token, context: remoteTestContext("runtime-session"),
      fetchFn: async (input, init) => {
        const url = new URL(String(input));
        requests.push({ url, init });
        if (url.pathname.endsWith("/api/host-capabilities")) return Response.json({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
        throw new Error(`非预期请求：${url}`);
      },
      webSocketFactory: (url, offered) => {
        socketUrl = url;
        protocols = offered;
        return { readyState: 1, close() { closed += 1; }, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
      },
    });
    await host.start(() => undefined);
    const socket = new URL(socketUrl);
    assert.equal(socket.protocol, "wss:");
    assert.equal(socket.pathname, "/game/ws");
    assert.equal(socket.searchParams.get("session_id"), "runtime-session");
    assert.equal(socket.searchParams.has("token"), false);
    assert.deepEqual(protocols, ["stock-game", `stock-game.auth.${Array.from(new TextEncoder().encode(token), (byte) => byte.toString(16).padStart(2, "0")).join("")}`]);
    await host.dispose();
    assert.equal(closed, 1);
    assert.equal(requests.length, 1);
    assert.equal(requests[0].init?.method, "GET");
    assert.equal(new Headers(requests[0].init?.headers).get("authorization"), `Bearer ${token}`);
    assert.equal(requests.some(({ url, init }) => url.pathname.endsWith("/api/new") || url.pathname.endsWith("/api/running") || init?.method === "DELETE"), false);
  }
});

test("入场资金控制只修改未来入场设置，包含generation且不join或刷新账户", async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const host = await production.createRemoteHost(DEFAULT_SETUP, 1n, {
    token: "identity-token", context: remoteTestContext(),
    fetchFn: async (input, init) => {
      requests.push({ url: String(input), init });
      if (String(input).includes("/api/host-capabilities")) return Response.json({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
      if (String(input).includes("/api/market/context")) return Response.json(remoteTestContext());
      return new Response(null, { status: 204 });
    },
  });
  try {
    assert.equal(typeof host.setAdmissionCash, "function");
    await host.setAdmissionCash!("1000000000000");
    assert.deepEqual(JSON.parse(String(requests[1].init?.body)), { session_id: "session-1", generation: "1", admission_cash: "1000000000000" });
    assert.ok(requests[1].url.endsWith("/api/market/admission-cash"));
    await assert.rejects(host.setAdmissionCash!("-1"), /入场资金/);
    assert.equal(requests.length, 3);
  } finally { await host.dispose(); }
});
