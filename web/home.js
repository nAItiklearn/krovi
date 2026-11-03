/* Krovi website: home page behaviour.
   the pill with eyes that follow the cursor · inline clips (lazy, muted, looped, only while visible) ·
   the live "third-party requests" counter.
   Everything is DOM work: this file makes no network requests. */
(function () {
  "use strict";

  var reduceMotion = !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  function $(sel, ctx) { return (ctx || document).querySelector(sel); }
  function $$(sel, ctx) { return Array.prototype.slice.call((ctx || document).querySelectorAll(sel)); }
  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  /* ================= the pill: eyes follow the cursor =================
     Mirrors the app's idle companion: the pupils glance toward the pointer (clamped, eased), blink every few
     seconds, and any hover / press wakes the pill, which then shows the strip it shows in the app (sample
     numbers here). Touch screens and idle pointers get a slow look-around instead. */
  function initPet() {
    var pet = $("[data-pet]");
    if (!pet) return;
    var screen = $("[data-pet-screen]") || pet.parentNode;
    var tx = 0, ty = 0, pending = false, lastMove = Date.now();
    var awakeTimer = null, driftTimer = null, blinkTimer = null;

    function px(em) { return em * (parseFloat(getComputedStyle(pet).fontSize) || 16); }
    function apply(x, y) {
      pet.style.setProperty("--ex", x.toFixed(2) + "px");
      pet.style.setProperty("--ey", y.toFixed(2) + "px");
    }
    function lookAt(clientX, clientY) {
      var r = pet.getBoundingClientRect();
      var dx = clientX - (r.left + r.width / 2);
      var dy = clientY - (r.top + r.height / 2);
      var d = Math.hypot(dx, dy) || 1;
      var k = Math.tanh(d / 220);            // far away saturates, close is subtle
      tx = (dx / d) * k * px(1.05);
      ty = (dy / d) * k * px(0.6);
    }
    function onMove(e) {
      lastMove = Date.now();
      if (pending) { lookAt(e.clientX, e.clientY); return; }
      pending = true;
      lookAt(e.clientX, e.clientY);
      requestAnimationFrame(function () { apply(tx, ty); pending = false; });
    }
    window.addEventListener("pointermove", function (e) { if (e.pointerType !== "touch") onMove(e); }, { passive: true });
    window.addEventListener("pointerdown", function (e) { lastMove = Date.now(); onMove(e); }, { passive: true });
    document.addEventListener("mouseleave", function () { tx = 0; ty = 0; apply(0, 0); });

    function wake(ms) {
      pet.classList.add("is-awake");
      screen.classList.add("is-awake");   // the sample toast under the pill
      clearTimeout(awakeTimer);
      awakeTimer = setTimeout(function () { pet.classList.remove("is-awake"); screen.classList.remove("is-awake"); }, ms);
    }
    pet.addEventListener("click", function () { wake(3400); });
    pet.addEventListener("pointerenter", function (e) { if (e.pointerType === "mouse") wake(2600); });
    pet.addEventListener("focus", function () { wake(3400); });

    if (reduceMotion) return; // follow the pointer, but no blinking or wandering on its own

    function blink() {
      pet.classList.add("is-blink");
      setTimeout(function () { pet.classList.remove("is-blink"); }, 180);
      blinkTimer = setTimeout(blink, 2600 + Math.random() * 3800);
    }
    blinkTimer = setTimeout(blink, 1800);

    function drift() {
      if (Date.now() - lastMove > 4200 && !pet.classList.contains("is-awake")) {
        var a = Math.random() * Math.PI * 2, m = 0.35 + Math.random() * 0.65;
        apply(Math.cos(a) * m * px(1.0), Math.sin(a) * m * px(0.5));
      }
      driftTimer = setTimeout(drift, 1800 + Math.random() * 1800);
    }
    driftTimer = setTimeout(drift, 3000);

    // stop all timers while the tab is hidden or the pill is off screen
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (entries) {
        if (entries[0].isIntersecting) return;
        clearTimeout(driftTimer); clearTimeout(blinkTimer);
        var back = new IntersectionObserver(function (e2) {
          if (!e2[0].isIntersecting) return;
          back.disconnect();
          blinkTimer = setTimeout(blink, 1200); driftTimer = setTimeout(drift, 2000);
        });
        back.observe(screen);
      }).observe(screen);
    }
  }

  /* ================= inline clips =================
     preload="none" + a poster means nothing downloads until the clip is on screen. Reduced motion or Data
     Saver: the poster stays and nothing plays until the visitor presses play. A pause button covers WCAG 2.2.2.
     Without JS the native controls stay on. */
  function initClips() {
    var saver = !!(navigator.connection && navigator.connection.saveData);
    var auto = !(reduceMotion || saver) && "IntersectionObserver" in window;
    $$("[data-clip]").forEach(function (fig) {
      var v = $("video", fig), btn = $(".clip-toggle", fig);
      if (!v) return;
      v.removeAttribute("controls");
      var inView = false, userPaused = !auto;
      function play() { var p = v.play(); if (p && p.catch) p.catch(function () {}); }
      function setBtn() {
        if (!btn) return;
        btn.setAttribute("aria-pressed", userPaused ? "true" : "false");
        btn.setAttribute("aria-label", userPaused ? "Play this clip" : "Pause this clip");
      }
      function sync() { if (inView && !userPaused && !document.hidden) play(); else v.pause(); }
      setBtn();
      // Captions: the clip's own controls are hidden, so offer a CC button (off until asked for; the text is also in "Describe this video").
      var track = v.textTracks && v.textTracks.length ? v.textTracks[0] : null;
      if (track && btn) {
        track.mode = "disabled";
        var cc = el("button", "clip-cc", "CC");
        cc.type = "button";
        cc.setAttribute("aria-pressed", "false");
        cc.setAttribute("aria-label", "Show captions");
        btn.parentNode.insertBefore(cc, btn);
        cc.addEventListener("click", function () {
          var on = track.mode !== "showing";
          track.mode = on ? "showing" : "disabled";
          cc.setAttribute("aria-pressed", on ? "true" : "false");
          cc.setAttribute("aria-label", on ? "Hide captions" : "Show captions");
        });
      }
      if (auto) {
        new IntersectionObserver(function (entries) { inView = entries[0].isIntersecting; sync(); }, { threshold: 0.35 }).observe(fig);
        document.addEventListener("visibilitychange", sync);
      }
      if (btn) btn.addEventListener("click", function () {
        userPaused = !userPaused;
        setBtn();
        if (auto) sync(); else if (userPaused) v.pause(); else play();
      });
    });
  }

  /* ================= illustrations: only animate while on screen ================= */
  function initIllos() {
    var illos = $$(".illo");
    if (!illos.length || !("IntersectionObserver" in window)) { illos.forEach(function (n) { n.classList.add("is-live"); }); return; }
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) { en.target.classList.toggle("is-live", en.isIntersecting && !document.hidden); });
    }, { threshold: 0.2 });
    illos.forEach(function (n) { io.observe(n); });
    document.addEventListener("visibilitychange", function () {
      if (document.hidden) illos.forEach(function (n) { n.classList.remove("is-live"); });
    });
  }

  /* ================= privacy: count requests to other origins ================= */
  function initNetCheck() {
    var box = $("[data-netcheck]");
    if (!box) return;
    var num = $("[data-net-third]", box), list = $("[data-net-list]", box);
    function measure() {
      var origins = {}, total = 0;
      var entries = (window.performance && performance.getEntriesByType) ? performance.getEntriesByType("resource") : [];
      entries.forEach(function (e) {
        try { var o = new URL(e.name, location.href).origin; origins[o] = (origins[o] || 0) + 1; total++; } catch (x) { /* ignore */ }
      });
      var third = Object.keys(origins).filter(function (o) { return o !== location.origin; });
      num.textContent = String(third.length);
      list.textContent = "";
      list.appendChild(el("li", "", "this page: " + total + " requests, all to " + location.host));
      third.forEach(function (o) { list.appendChild(el("li", "", "other origin: " + o)); });
    }
    if (document.readyState === "complete") setTimeout(measure, 300); else window.addEventListener("load", function () { setTimeout(measure, 300); });
  }

  function init() {
    initPet();
    initClips();
    initIllos();
    initNetCheck();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
