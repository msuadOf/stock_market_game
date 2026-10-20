const resourceScope = new URL(self.registration.scope);

self.addEventListener("install", (event) => event.waitUntil(self.skipWaiting()));
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));
self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== resourceScope.origin || !url.pathname.startsWith(resourceScope.pathname)) return;
  const relative = url.pathname.slice(resourceScope.pathname.length);
  if (relative !== "" && relative !== "index.html" && !relative.startsWith("assets/") && !["favicon.svg", "icons.svg", "build-info.json", "LICENSE"].includes(relative)) return;
  if (request.cache === "only-if-cached" && request.mode !== "same-origin") return;
  event.respondWith(fetch(request).then((response) => {
    if (response.status === 0) return response;
    const headers = new Headers(response.headers);
    headers.set("Cross-Origin-Opener-Policy", "same-origin");
    headers.set("Cross-Origin-Embedder-Policy", "require-corp");
    return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
  }));
});
