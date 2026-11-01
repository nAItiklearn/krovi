/* Krovi site: runs synchronously in <head> so the saved theme is applied before first paint
   (no flash), and tags <html> as JS-enabled so scroll-reveal styles only apply when JS runs.
   Self-hosted, no network. */
(function () {
  var d = document.documentElement;
  d.classList.add("js");
  try {
    var t = localStorage.getItem("krovi-theme");
    if (t === "light" || t === "dark") d.setAttribute("data-theme", t);
  } catch (e) {}
})();
