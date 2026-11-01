/* Krovi website — shared behaviour. Vanilla JS, no dependencies, no network, no storage
   except the theme preference (localStorage, wrapped in try/catch).
   theme · mobile nav · nav border on scroll · scroll reveal · count-up · copy buttons ·
   download button states · docs scroll-spy. */
(function () {
  "use strict";

  var root = document.documentElement;
  var reduceMotion = !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  function $(sel, ctx) { return (ctx || document).querySelector(sel); }
  function $$(sel, ctx) { return Array.prototype.slice.call((ctx || document).querySelectorAll(sel)); }

  /* ---------------- theme ---------------- */
  function isDark() {
    var a = root.getAttribute("data-theme");
    if (a === "dark") return true;
    if (a === "light") return false;
    return !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);
  }
  function syncDemoFrames() {
    // The embedded demo is same-origin, so keep it in step with the site theme.
    $$("iframe[data-demo-frame]").forEach(function (f) {
      try {
        var d = f.contentDocument;
        if (d && d.documentElement) d.documentElement.setAttribute("data-theme", isDark() ? "dark" : "light");
      } catch (e) { /* not available yet: harmless */ }
    });
  }
  function syncThemeButton() {
    var btn = $(".theme-toggle");
    if (!btn) return;
    var dark = isDark();
    btn.setAttribute("aria-pressed", String(dark));
    btn.setAttribute("aria-label", dark ? "Switch to light theme" : "Switch to dark theme");
  }
  function wireTheme() {
    var btn = $(".theme-toggle");
    syncThemeButton();
    if (!btn) return;
    btn.addEventListener("click", function () {
      var next = isDark() ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try { localStorage.setItem("krovi-theme", next); } catch (e) { /* storage blocked: still works this visit */ }
      syncThemeButton();
      syncDemoFrames();
    });
    if (window.matchMedia) {
      var mq = window.matchMedia("(prefers-color-scheme: dark)");
      var onChange = function () { syncThemeButton(); syncDemoFrames(); };
      if (mq.addEventListener) mq.addEventListener("change", onChange);
    }
  }
  window.krovi = window.krovi || {};
  window.krovi.syncDemoFrames = syncDemoFrames;

  /* ---------------- nav ---------------- */
  function wireNav() {
    var nav = $(".nav");
    var toggle = $(".nav-toggle");
    var links = $("#nav-links");
    if (nav) {
      var onScroll = function () { nav.classList.toggle("scrolled", window.scrollY > 6); };
      onScroll();
      window.addEventListener("scroll", onScroll, { passive: true });
    }
    if (!toggle || !links) return;
    function setOpen(open) {
      links.setAttribute("data-open", String(open));
      toggle.setAttribute("aria-expanded", String(open));
      toggle.setAttribute("aria-label", open ? "Close menu" : "Open menu");
    }
    toggle.addEventListener("click", function () { setOpen(links.getAttribute("data-open") !== "true"); });
    links.addEventListener("click", function (e) { if (e.target.closest("a")) setOpen(false); });
    document.addEventListener("keydown", function (e) { if (e.key === "Escape") setOpen(false); });
    document.addEventListener("click", function (e) {
      if (links.getAttribute("data-open") === "true" && !e.target.closest(".nav")) setOpen(false);
    });
  }

  /* ---------------- scroll reveal ---------------- */
  function wireReveal() {
    var els = $$(".reveal");
    if (!els.length) return;
    if (reduceMotion || !("IntersectionObserver" in window)) {
      els.forEach(function (el) { el.classList.add("in"); });
      return;
    }
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) {
          en.target.classList.add("in");
          io.unobserve(en.target);
          if (en.target.hasAttribute("data-count-wrap")) countUp(en.target);
        }
      });
    }, { rootMargin: "0px 0px -7% 0px", threshold: 0.06 });
    els.forEach(function (el) { io.observe(el); });
  }

  /* ---------------- count-up ---------------- */
  function countUp(scope) {
    $$("[data-count]", scope).forEach(function (el) {
      var to = parseInt(el.getAttribute("data-count"), 10);
      if (isNaN(to) || reduceMotion) return;
      var start = null, dur = 1100;
      function frame(t) {
        if (start === null) start = t;
        var p = Math.min(1, (t - start) / dur);
        var eased = 1 - Math.pow(1 - p, 3);
        el.textContent = String(Math.round(to * eased));
        if (p < 1) requestAnimationFrame(frame);
      }
      el.textContent = "0";
      requestAnimationFrame(frame);
    });
  }
  function wireCountUp() {
    var groups = $$(".stats-grid");
    groups.forEach(function (g) { g.setAttribute("data-count-wrap", ""); });
  }

  /* ---------------- copy buttons ---------------- */
  function copyText(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise(function (resolve, reject) {
      var ta = document.createElement("textarea");
      ta.value = text; ta.setAttribute("readonly", ""); ta.className = "sr-only";
      document.body.appendChild(ta); ta.select();
      try { document.execCommand("copy") ? resolve() : reject(); } catch (e) { reject(e); }
      document.body.removeChild(ta);
    });
  }
  function wireCopy() {
    document.addEventListener("click", function (e) {
      var b = e.target.closest(".copy-btn");
      if (!b) return;
      var text = b.getAttribute("data-copy");
      if (text == null) {
        var block = b.closest(".codeblock");
        var pre = block && $("pre", block);
        text = pre ? pre.textContent.replace(/\s+$/, "") : "";
      }
      var label = $("span", b);
      copyText(text).then(function () {
        b.setAttribute("data-copied", "true");
        if (label) label.textContent = "Copied";
        setTimeout(function () { b.removeAttribute("data-copied"); if (label) label.textContent = "Copy"; }, 1800);
      }, function () { if (label) label.textContent = "Press Ctrl+C"; });
    });
  }

  /* ---------------- download button: hover sheen, press, busy, done ---------------- */
  function wireDownload() {
    $$("[data-download]").forEach(function (btn) {
      var status = document.getElementById(btn.getAttribute("aria-describedby"));
      var timer;
      btn.addEventListener("pointermove", function (e) {
        var r = btn.getBoundingClientRect();
        btn.style.setProperty("--mx", (e.clientX - r.left) + "px");
        btn.style.setProperty("--my", (e.clientY - r.top) + "px");
      });
      btn.addEventListener("click", function () {
        if (btn.getAttribute("data-state") === "busy") return;
        clearTimeout(timer);
        btn.setAttribute("data-state", "busy");
        if (status) status.textContent = "Opening the download…";
        timer = setTimeout(function () {
          btn.setAttribute("data-state", "done");
          if (status) status.textContent = "Done. Your download opened in a new tab.";
          timer = setTimeout(function () {
            btn.removeAttribute("data-state");
            if (status) status.textContent = "";
          }, 5200);
        }, reduceMotion ? 120 : 950);
      });
    });
  }

  /* ---------------- docs: table-of-contents scroll-spy ---------------- */
  function wireToc() {
    var links = $$(".toc a[href^='#']");
    if (!links.length || !("IntersectionObserver" in window)) return;
    var map = {};
    links.forEach(function (a) {
      var id = decodeURIComponent(a.getAttribute("href").slice(1));
      var sec = document.getElementById(id);
      if (sec) map[id] = sec;
    });
    function setActive(id) {
      links.forEach(function (a) {
        var on = a.getAttribute("href") === "#" + id;
        a.classList.toggle("active", on);
        if (on) a.setAttribute("aria-current", "location"); else a.removeAttribute("aria-current");
      });
    }
    var visible = {};
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) { visible[en.target.id] = en.isIntersecting ? en.boundingClientRect.top : null; });
      var best = null;
      Object.keys(visible).forEach(function (id) {
        if (visible[id] === null) return;
        if (best === null || visible[id] < visible[best]) best = id;
      });
      if (best) setActive(best);
    }, { rootMargin: "-12% 0px -72% 0px", threshold: 0 });
    Object.keys(map).forEach(function (id) { io.observe(map[id]); });
  }

  function init() {
    wireTheme();
    wireNav();
    wireCountUp();
    wireReveal();
    wireCopy();
    wireDownload();
    wireToc();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
