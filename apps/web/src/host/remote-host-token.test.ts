import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer, type ViteDevServer } from "vite";
import type { SessionSetup } from "../types/engine.ts";

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

function requestsFor(sessionToken?: string) {
  const requests: { url: string; init?: RequestInit }[] = [];
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    requests.push({ url, init });
    if (url.endsWith("/api/new")) return new Response(JSON.stringify({
      session_id: "runtime-session",
      ...(sessionToken === undefined ? {} : { session_token: sessionToken }),
    }));
    if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }));
    if (url.endsWith("/api/running")) return new Response(null);
    if (init?.method === "DELETE") return new Response(null);
    throw new Error(`Unexpected mock request: ${url}`);
  }) as typeof fetch;
  return { requests, fetchFn };
}

async function assertAuthorization(module: RemoteModule, expected: string, returnedToken?: string, explicitToken?: string) {
  const { requests, fetchFn } = requestsFor(returnedToken);
  let socketUrl = "";
  let protocols: string[] = [];
  const host = await module.createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "https://new-server.example/game",
    fetchFn,
    ...(explicitToken === undefined ? {} : { token: explicitToken }),
    webSocketFactory: (url, offered) => {
      socketUrl = url;
      protocols = offered;
      return { readyState: 1, close() {}, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
    },
  });
  try {
    const capabilityRequest = requests.find(({ url }) => url.includes("/api/host-capabilities?"));
    assert.ok(capabilityRequest);
    assert.equal(new Headers(capabilityRequest.init?.headers).get("authorization"), `Bearer ${expected}`);
    assert.equal(new Headers(requests[0].init?.headers).has("authorization"), false);
    assert.equal(requests[0].url, "https://new-server.example/game/api/new");
    await host.start(() => undefined);
    const socket = new URL(socketUrl);
    assert.equal(socket.protocol, "wss:");
    assert.equal(socket.pathname, "/game/ws");
    assert.equal(socket.searchParams.get("session_id"), "runtime-session");
    assert.equal(socket.searchParams.has("token"), false);
    assert.deepEqual(protocols, ["stock-game", `stock-game.auth.${Array.from(new TextEncoder().encode(expected), (byte) => byte.toString(16).padStart(2, "0")).join("")}`]);
  } finally {
    await host.dispose();
  }
  await new Promise((resolve) => setImmediate(resolve));
  const deletion = requests.find(({ init }) => init?.method === "DELETE");
  assert.ok(deletion);
  assert.equal(new Headers(deletion.init?.headers).get("authorization"), `Bearer ${expected}`);
}

test("生产任意 Server 使用该会话返回 token，不使用编译 legacy token", async () => {
  await assertAuthorization(production, "returned-session-token", "returned-session-token");
});

test("显式 options.token 在生产和开发均保持优先，既有 local-player fallback 不变", async () => {
  for (const module of [production, development]) {
    await assertAuthorization(module, "explicit-runtime-token", "returned-session-token", "explicit-runtime-token");
  }
  await assertAuthorization(production, "local-player");
});

test("仅开发构建保留 legacy token，空显式 token 仍显式拒绝而非 fallback", async () => {
  await assertAuthorization(development, "compiled-legacy-token", "returned-session-token");
  for (const module of [production, development]) {
    const { fetchFn, requests } = requestsFor("returned-session-token");
    await assert.rejects(module.createRemoteHost({} as SessionSetup, 1n, {
      baseUrl: "https://new-server.example/game", fetchFn, token: "",
    }), /缺少授权 token/);
    assert.equal(requests.length, 1);
  }
});
