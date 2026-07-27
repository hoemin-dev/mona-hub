const CACHE_NAME = "mona-hub-v1";

self.addEventListener("install", () => {
  console.log("MONA Hub service worker installed");
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("fetch", () => {
  // 설치 가능 여부 확인을 위한 최소 fetch handler
});