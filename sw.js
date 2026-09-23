const cachePrefix = 'sekai-pwa-';
const cacheName = `${cachePrefix}network-first-v2`;
const legacyCacheNames = new Set(['egui-template-pwa']);
const filesToCache = [
  './',
  './index.html',
  './sekai.js',
  './sekai_bg.wasm',
];

/* Seed an offline fallback, then activate this release without waiting for old tabs. */
self.addEventListener('install', function (e) {
  e.waitUntil(
    caches
      .open(cacheName)
      .then(function (cache) {
        return cache.addAll(filesToCache);
      })
      .then(function () {
        return self.skipWaiting();
      })
  );
});

/* Retire only Sekai caches owned by an older release before taking control. */
self.addEventListener('activate', function (e) {
  e.waitUntil(
    caches
      .keys()
      .then(function (names) {
        return Promise.all(
          names
            .filter(function (name) {
              return (
                name !== cacheName &&
                (name.startsWith(cachePrefix) || legacyCacheNames.has(name))
              );
            })
            .map(function (name) {
              return caches.delete(name);
            })
        );
      })
      .then(function () {
        return self.clients.claim();
      })
  );
});

/* Always ask the server for the current release; use the cache only offline. */
self.addEventListener('fetch', function (e) {
  if (e.request.method !== 'GET') {
    return;
  }
  const url = new URL(e.request.url);
  if (url.origin !== self.location.origin) {
    return;
  }

  e.respondWith(
    fetch(e.request, { cache: 'no-cache' })
      .then(function (response) {
        if (!response || !response.ok) {
          return response;
        }
        const current = response.clone();
        return caches.open(cacheName).then(function (cache) {
          return cache.put(e.request, current).then(function () {
            return response;
          });
        });
      })
      .catch(function () {
        return caches.open(cacheName).then(function (cache) {
          return cache.match(e.request).then(function (cached) {
            if (cached) {
              return cached;
            }
            if (e.request.mode === 'navigate') {
              return cache.match('./index.html');
            }
            return undefined;
          });
        });
      })
  );
});
