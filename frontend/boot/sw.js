// APP-92: menger's service worker — the next load paints at once.
//
// frontend/boot/split.mjs writes it into the build (the version and the
// files below); the shell serves it at /sw.js, no-cache, so a new build's
// is seen on the next load. Registered by the app's page and by the
// sign-in page, so the app downloads while the reader signs in.
//
// - The build's files (the app's script and wasm, its stylesheet, the
//   faces) are kept on install, one cache per build; a new build's
//   replaces them, the build before it kept a while for a page still
//   running it.
// - The app's page (`/`, `/app/`) is answered from the cache at once and
//   fetched again behind it (stale-while-revalidate): a reload draws the
//   boot sheet without waiting on the network. When the fetch says the
//   session is gone (a redirect to the sign-in page, a 401), the kept page
//   is dropped and every open page reloads, onto the sign-in page.
// - Signing out (`POST /logout`) drops the kept page.
//
// Only GETs of this origin are touched; the socket, the sign-in pages and
// everything else go to the network as if there were no worker.
const VERSION = "__VERSION__";
const KEEP = ["__KEEP__"];
const BUILD = `menger-build-${VERSION}`;
const PAGE = "menger-page";
const PAGE_KEY = "/";
const APP_PAGES = new Set(["/", "/app", "/app/", "/app/index.html"]);
const keep = new Set(KEEP);

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(BUILD).then((c) => c.addAll(KEEP)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    (async () => {
      // Names sort by build time: this build and the one before it stay.
      const builds = (await caches.keys()).filter((n) => n.startsWith("menger-build-") && n !== BUILD).sort();
      await Promise.all(builds.slice(0, -1).map((n) => caches.delete(n)));
      await self.clients.claim();
    })(),
  );
});

async function tell(message) {
  for (const c of await self.clients.matchAll({ type: "window" })) c.postMessage(message);
}

// The page again from the network; kept when it is the app's.
async function refresh(req) {
  const cache = await caches.open(PAGE);
  try {
    const res = await fetch(req);
    if (res.ok && res.type === "basic" && (res.headers.get("content-type") || "").includes("text/html")) {
      await cache.put(PAGE_KEY, res.clone());
    } else if (res.type === "opaqueredirect" || res.status === 401 || res.status === 403 || res.status === 303) {
      if (await cache.delete(PAGE_KEY)) await tell("menger:signed-out");
    }
    return res;
  } catch (err) {
    return null;
  }
}

self.addEventListener("fetch", (e) => {
  const req = e.request;
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) return;
  if (req.method === "POST" && url.pathname === "/logout") {
    e.waitUntil(caches.delete(PAGE));
    return;
  }
  if (req.method !== "GET") return;
  if (req.mode === "navigate" && APP_PAGES.has(url.pathname)) {
    e.respondWith(
      (async () => {
        const kept = await (await caches.open(PAGE)).match(PAGE_KEY);
        const fresh = refresh(req);
        if (kept) {
          e.waitUntil(fresh);
          return kept;
        }
        return (await fresh) || fetch(req);
      })(),
    );
    return;
  }
  if (keep.has(url.pathname) || (url.pathname.startsWith("/app/") && /^(menger-app-|app-)/.test(url.pathname.slice(5)))) {
    e.respondWith(
      (async () => {
        const hit = await caches.match(url.pathname);
        if (hit) return hit;
        const res = await fetch(req);
        // A page from an older build than the server's asks for its files
        // after they are gone: reload it onto the build there is.
        if (res.status === 404) {
          await caches.delete(PAGE);
          await tell("menger:stale");
        }
        return res;
      })(),
    );
  }
});
