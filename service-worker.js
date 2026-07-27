const CACHE_NAME = "mona-hub-v1";

const APP_SHELL = [
  "/",
  "/index.html",
  "/style.css",
  "/app.js",
  "/manifest.webmanifest",
  "/icons/icon-192.png",
  "/icons/icon-512.png"
];


/**
 * 설치
 */
self.addEventListener("install", event => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then(cache => cache.addAll(APP_SHELL))
      .then(() => self.skipWaiting())
  );
});


/**
 * 활성화
 */
self.addEventListener("activate", event => {
  event.waitUntil(
    caches
      .keys()
      .then(cacheNames => {
        return Promise.all(
          cacheNames
            .filter(cacheName => cacheName !== CACHE_NAME)
            .map(cacheName => caches.delete(cacheName))
        );
      })
      .then(() => self.clients.claim())
  );
});


/**
 * 네트워크 요청
 */
self.addEventListener("fetch", event => {
  const request = event.request;

  if (request.method !== "GET") {
    return;
  }

  const requestUrl = new URL(request.url);

  /*
   * manualkit.pages.dev는 다른 도메인이므로
   * MONA Hub 캐시에서 관리하지 않음
   */
  if (requestUrl.origin !== self.location.origin) {
    return;
  }

  event.respondWith(
    fetch(request)
      .then(response => {
        const responseClone = response.clone();

        caches
          .open(CACHE_NAME)
          .then(cache => cache.put(request, responseClone));

        return response;
      })
      .catch(() => {
        return caches.match(request);
      })
  );
});