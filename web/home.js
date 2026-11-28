/* Krovi website: live third-party request counter.
   Reads the browser's own resource timing entries. It makes no requests,
   stores nothing and sends nothing. */
(function () {
  "use strict";

  var count = document.querySelector("[data-net-third]");
  if (!count) return;

  var list = document.querySelector("[data-net-list]");
  var origins = [];
  var seen = Object.create(null);

  function add(url) {
    if (!url) return;
    var parsed;
    try {
      parsed = new URL(url, window.location.href);
    } catch (error) {
      return;
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;
    if (parsed.origin === window.location.origin) return;
    if (seen[parsed.origin]) return;

    seen[parsed.origin] = true;
    origins.push(parsed.origin);
    count.textContent = String(origins.length);

    if (list) {
      var item = document.createElement("li");
      item.textContent = parsed.origin;
      list.appendChild(item);
    }
  }

  function scan() {
    if (!window.performance || !performance.getEntriesByType) return;
    performance.getEntriesByType("resource").forEach(function (entry) {
      add(entry.name);
    });
  }

  scan();

  if ("PerformanceObserver" in window) {
    try {
      new PerformanceObserver(function (entries) {
        entries.getEntries().forEach(function (entry) {
          add(entry.name);
        });
      }).observe({ type: "resource", buffered: true });
    } catch (error) {
      /* The initial scan still covers browsers without this observer type. */
    }
  }
})();
