// Response-header approach: coi-serviceworker, Copyright (c) 2021 Guido Zuidhof.
// MIT notice: licenses/coi-serviceworker-MIT.txt. Project-specific adaptation.
// Static hosts cannot set COOP/COEP. Wrap same-origin responses without caching
// or buffering their bodies; streaming movies and the LUT keep their own caches.
export function isolatedResponse(response) {
  if (response.status === 0) return response;
  const headers = new Headers(response.headers);
  headers.set('Cross-Origin-Opener-Policy', 'same-origin');
  headers.set('Cross-Origin-Embedder-Policy', 'require-corp');
  headers.set('Cross-Origin-Resource-Policy', 'same-origin');
  return new Response(response.body, {
    status: response.status, statusText: response.statusText, headers,
  });
}

export function inScope(request, scope) {
  const url = new URL(request.url), root = new URL(scope);
  return request.method === 'GET' && url.origin === root.origin &&
    url.pathname.startsWith(root.pathname) &&
    !(request.cache === 'only-if-cached' && request.mode !== 'same-origin');
}

if (typeof ServiceWorkerGlobalScope !== 'undefined' && self instanceof ServiceWorkerGlobalScope) {
  self.addEventListener('install', event => event.waitUntil(self.skipWaiting()));
  self.addEventListener('activate', event => event.waitUntil(self.clients.claim()));
  self.addEventListener('fetch', event => {
    if (inScope(event.request, self.registration.scope))
      event.respondWith(fetch(event.request).then(isolatedResponse));
  });
}

function deadline(promise, env) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = env.setTimeout(() => reject(new Error('Isolation activation timed out')), 5000);
    }),
  ]).finally(() => env.clearTimeout(timer));
}

function controlled(serviceWorker, url, env) {
  const owns = () => serviceWorker.controller?.scriptURL === url.href;
  if (owns()) return Promise.resolve();
  let listener;
  const ready = new Promise(resolve => {
    listener = () => { if (owns()) resolve(); };
    serviceWorker.addEventListener('controllerchange', listener);
    listener();
  });
  return deadline(ready, env).finally(() =>
    serviceWorker.removeEventListener('controllerchange', listener));
}

// Resolve before application startup. Native headers need no worker; static
// hosting needs a controlled navigation. Unsupported browsers keep the fallback.
export async function ensureIsolation(env = globalThis, url = new URL(import.meta.url)) {
  const {navigator, location} = env;
  const worker = navigator?.serviceWorker;
  const key = `mupic-isolation:${url.href}`;
  const ownsController = worker?.controller &&
    new URL(worker.controller.scriptURL).pathname === url.pathname;
  if (env.crossOriginIsolated) {
    try { env.sessionStorage.removeItem(key); } catch {}
    // A controlled Pages visit still checks for a new worker, without reloading
    // a running player or an in-progress conversion.
    if (ownsController) worker.register(url, {
      scope: new URL('./', url).href, type: 'module', updateViaCache: 'none',
    }).catch(() => {});
    return true;
  }
  if (!env.isSecureContext || !worker) return false;
  try {
    await deadline(worker.register(url, {
      scope: new URL('./', url).href, type: 'module', updateViaCache: 'none',
    }), env);
    await controlled(worker, url, env);
    // Storage must record the attempt before navigating. If storage is denied,
    // keep serial playback rather than repeatedly discarding the current page.
    if (env.sessionStorage.getItem(key)) return false;
    env.sessionStorage.setItem(key, '1');
    location.reload();
    // The old document must not start workers or load the LUT while reloading.
    return new Promise(() => {});
  } catch {
    return false;
  }
}
