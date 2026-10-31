/* Krovi — vanilla JS, no external dependencies, no trackers, no network.
   1. Theme toggle (light / dark) persisted in localStorage (try/catch).
   2. Mobile nav open/close.
   3. Interactive "live preview": tabs + timer, world clocks, calculator,
      clipboard filter, unit converter. All front-end, no backend.
   4. Docs table-of-contents scroll-spy (only on the docs page). */
(function () {
  "use strict";

  var root = document.documentElement;
  var STORAGE_KEY = "krovi-theme";
  var reduceMotion = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ----- Theme ----- */
  function storedTheme() {
    try { return window.localStorage.getItem(STORAGE_KEY); } catch (e) { return null; }
  }
  function saveTheme(value) {
    try { window.localStorage.setItem(STORAGE_KEY, value); } catch (e) { /* blocked storage: toggle still works this session */ }
  }
  var saved = storedTheme();
  if (saved === "light" || saved === "dark") root.setAttribute("data-theme", saved);

  function currentlyDark() {
    var attr = root.getAttribute("data-theme");
    if (attr === "dark") return true;
    if (attr === "light") return false;
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  }
  function wireThemeToggle() {
    var btn = document.querySelector(".theme-toggle");
    if (!btn) return;
    function sync() {
      var dark = currentlyDark();
      btn.setAttribute("aria-pressed", String(dark));
      btn.setAttribute("aria-label", dark ? "Switch to light theme" : "Switch to dark theme");
    }
    sync();
    btn.addEventListener("click", function () {
      var next = currentlyDark() ? "light" : "dark";
      root.setAttribute("data-theme", next);
      saveTheme(next);
      sync();
    });
  }

  /* ----- Mobile nav ----- */
  function wireNav() {
    var toggle = document.querySelector(".nav-toggle");
    var links = document.getElementById("nav-links");
    if (!toggle || !links) return;
    function setOpen(open) {
      links.setAttribute("data-open", String(open));
      toggle.setAttribute("aria-expanded", String(open));
      toggle.setAttribute("aria-label", open ? "Close menu" : "Open menu");
    }
    setOpen(false);
    toggle.addEventListener("click", function () {
      setOpen(links.getAttribute("data-open") !== "true");
    });
    links.addEventListener("click", function (e) { if (e.target.closest("a")) setOpen(false); });
    document.addEventListener("keydown", function (e) { if (e.key === "Escape") setOpen(false); });
  }

  /* ----- Live preview: tablist ----- */
  function wireLiveTabs() {
    var rail = document.querySelector(".live-rail");
    if (!rail) return;
    var tabs = Array.prototype.slice.call(rail.querySelectorAll('[role="tab"]'));

    function select(tab) {
      tabs.forEach(function (t) {
        var on = t === tab;
        t.setAttribute("aria-selected", String(on));
        t.tabIndex = on ? 0 : -1;
        var panel = document.getElementById(t.getAttribute("aria-controls"));
        if (panel) panel.hidden = !on;
      });
    }
    tabs.forEach(function (tab, i) {
      tab.addEventListener("click", function () { select(tab); });
      tab.addEventListener("keydown", function (e) {
        var next;
        if (e.key === "ArrowDown" || e.key === "ArrowRight") next = tabs[(i + 1) % tabs.length];
        else if (e.key === "ArrowUp" || e.key === "ArrowLeft") next = tabs[(i - 1 + tabs.length) % tabs.length];
        else if (e.key === "Home") next = tabs[0];
        else if (e.key === "End") next = tabs[tabs.length - 1];
        if (next) { e.preventDefault(); select(next); next.focus(); }
      });
    });
  }

  /* ----- Live preview: focus timer ----- */
  function wireTimer() {
    var view = document.getElementById("view-timer");
    if (!view) return;
    var display = view.querySelector("[data-timer-display]");
    var state = view.querySelector("[data-timer-state]");
    var ring = view.querySelector("[data-timer-ring]");
    var announce = view.querySelector("[data-timer-announce]");
    var toggleBtn = view.querySelector('[data-timer-action="toggle"]');
    var resetBtn = view.querySelector('[data-timer-action="reset"]');
    var presets = Array.prototype.slice.call(view.querySelectorAll("[data-preset]"));
    var CIRC = 2 * Math.PI * 66;

    var total = 300, remaining = 300, running = false, tid = null;

    function fmt(s) {
      s = Math.max(0, Math.round(s));
      var m = Math.floor(s / 60), r = s % 60;
      return (m < 10 ? "0" : "") + m + ":" + (r < 10 ? "0" : "") + r;
    }
    function label(s) {
      var m = Math.round(s / 60);
      return m >= 1 ? (m + " minute" + (m === 1 ? "" : "s")) : (s + " seconds");
    }
    function render() {
      display.textContent = fmt(remaining);
      if (ring) {
        var p = total > 0 ? remaining / total : 0;
        ring.setAttribute("stroke-dashoffset", String(CIRC * (1 - p)));
      }
    }
    function setState(txt) { state.textContent = txt; }
    function stop() { if (tid) { clearInterval(tid); tid = null; } running = false; toggleBtn.textContent = "Start"; }

    function tick() {
      remaining -= 1;
      if (remaining <= 0) {
        remaining = 0; render(); stop();
        display.classList.add("ringing");
        setState("Time's up");
        if (announce) announce.textContent = "Timer finished.";
        return;
      }
      render();
    }
    function start() {
      if (remaining <= 0) return;
      running = true; toggleBtn.textContent = "Pause";
      display.classList.remove("ringing");
      setState("Running · " + fmt(remaining) + " left");
      tid = setInterval(tick, 1000);
    }
    function pause() { stop(); setState("Paused · " + fmt(remaining) + " left"); }
    function reset() {
      stop(); remaining = total; display.classList.remove("ringing");
      render(); setState("Ready · " + label(total));
    }

    toggleBtn.addEventListener("click", function () { running ? pause() : start(); });
    resetBtn.addEventListener("click", reset);
    presets.forEach(function (chip) {
      chip.addEventListener("click", function () {
        presets.forEach(function (c) { c.setAttribute("aria-pressed", String(c === chip)); });
        total = parseInt(chip.getAttribute("data-preset"), 10) || 300;
        reset();
      });
    });
    render();
  }

  /* ----- Live preview: world clocks ----- */
  function wireClocks() {
    var list = document.querySelector("[data-clock-list]");
    if (!list) return;
    var items = Array.prototype.slice.call(list.querySelectorAll("[data-tz]"));

    function parts(tz) {
      var opts = { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit", weekday: "short", day: "2-digit" };
      if (tz && tz !== "local") opts.timeZone = tz;
      try {
        var f = new Intl.DateTimeFormat("en-GB", opts).formatToParts(new Date());
        var o = {};
        f.forEach(function (p) { o[p.type] = p.value; });
        return o;
      } catch (e) { return null; }
    }
    var localDay = new Date().getDate();

    function update() {
      var nowLocalDay = new Date().getDate();
      items.forEach(function (item) {
        var tz = item.getAttribute("data-tz");
        var p = parts(tz);
        var timeEl = item.querySelector("[data-clock-time]");
        var dayEl = item.querySelector("[data-clock-day]");
        if (!p) { if (timeEl) timeEl.textContent = "--:--:--"; return; }
        if (timeEl) timeEl.textContent = p.hour + ":" + p.minute + ":" + p.second;
        if (dayEl) {
          var d = parseInt(p.day, 10);
          if (tz === "local" || isNaN(d)) dayEl.textContent = "";
          else if (d === nowLocalDay) dayEl.textContent = p.weekday;
          else if (d === nowLocalDay + 1 || (nowLocalDay > 27 && d === 1)) dayEl.textContent = "+1d";
          else if (d === nowLocalDay - 1 || (d > 27 && nowLocalDay === 1)) dayEl.textContent = "−1d";
          else dayEl.textContent = p.weekday;
        }
        var h = parseInt(p.hour, 10), m = parseInt(p.minute, 10);
        var hHand = item.querySelector('[data-hand="h"]');
        var mHand = item.querySelector('[data-hand="m"]');
        if (hHand) hHand.setAttribute("transform", "rotate(" + ((h % 12) * 30 + m * 0.5) + " 17 17)");
        if (mHand) mHand.setAttribute("transform", "rotate(" + (m * 6) + " 17 17)");
      });
    }
    update();
    setInterval(update, 1000);
  }

  /* ----- Live preview: calculator (safe shunting-yard, no eval) ----- */
  function wireCalc() {
    var view = document.getElementById("view-calc");
    if (!view) return;
    var exprEl = view.querySelector("[data-calc-expr]");
    var resultEl = view.querySelector("[data-calc-result]");
    var pad = view.querySelector("[data-calc-pad]");
    var expr = "";

    var OPS = { "+": 1, "-": 1, "*": 2, "/": 2 };
    function tokenize(s) {
      var toks = [], i = 0;
      while (i < s.length) {
        var c = s[i];
        if (c === " ") { i++; continue; }
        if ("0123456789.".indexOf(c) !== -1) {
          var num = c; i++;
          while (i < s.length && "0123456789.".indexOf(s[i]) !== -1) { num += s[i]; i++; }
          toks.push({ t: "num", v: parseFloat(num) });
          continue;
        }
        if ("+-*/()".indexOf(c) !== -1) { toks.push({ t: c }); i++; continue; }
        throw new Error("bad");
      }
      return toks;
    }
    function toRPN(toks) {
      var out = [], ops = [], prev = null;
      for (var i = 0; i < toks.length; i++) {
        var tk = toks[i];
        if (tk.t === "num") { out.push(tk); }
        else if (tk.t === "(") { ops.push(tk); }
        else if (tk.t === ")") {
          while (ops.length && ops[ops.length - 1].t !== "(") out.push(ops.pop());
          if (!ops.length) throw new Error("paren");
          ops.pop();
        } else if (tk.t in OPS) {
          // unary minus/plus
          if ((tk.t === "-" || tk.t === "+") && (prev === null || prev.t === "(" || prev.t in OPS)) {
            out.push({ t: "num", v: 0 });
          }
          while (ops.length && ops[ops.length - 1].t in OPS && OPS[ops[ops.length - 1].t] >= OPS[tk.t]) out.push(ops.pop());
          ops.push(tk);
        }
        prev = tk;
      }
      while (ops.length) { var o = ops.pop(); if (o.t === "(") throw new Error("paren"); out.push(o); }
      return out;
    }
    function evalRPN(rpn) {
      var st = [];
      for (var i = 0; i < rpn.length; i++) {
        var tk = rpn[i];
        if (tk.t === "num") { st.push(tk.v); continue; }
        var b = st.pop(), a = st.pop();
        if (a === undefined || b === undefined) throw new Error("args");
        if (tk.t === "+") st.push(a + b);
        else if (tk.t === "-") st.push(a - b);
        else if (tk.t === "*") st.push(a * b);
        else if (tk.t === "/") { if (b === 0) throw new Error("div0"); st.push(a / b); }
      }
      if (st.length !== 1) throw new Error("expr");
      return st[0];
    }
    function compute(s) {
      var norm = s.replace(/×/g, "*").replace(/÷/g, "/").replace(/−/g, "-");
      var val = evalRPN(toRPN(tokenize(norm)));
      if (!isFinite(val)) throw new Error("inf");
      return Math.round((val + Number.EPSILON) * 1e9) / 1e9;
    }
    function tryResult() {
      if (!expr) { resultEl.textContent = "0"; return; }
      try { resultEl.textContent = String(compute(expr)); }
      catch (e) { resultEl.textContent = "…"; }
    }
    function renderExpr() { exprEl.textContent = expr || " "; }

    function press(key) {
      if (key === "clear") { expr = ""; renderExpr(); resultEl.textContent = "0"; return; }
      if (key === "back") { expr = expr.slice(0, -1); renderExpr(); tryResult(); return; }
      if (key === "=") {
        try { var r = compute(expr); expr = String(r); renderExpr(); resultEl.textContent = String(r); }
        catch (e) { resultEl.textContent = "Error"; }
        return;
      }
      expr += key;
      renderExpr();
      tryResult();
    }
    pad.addEventListener("click", function (e) {
      var b = e.target.closest("button[data-key]");
      if (b) press(b.getAttribute("data-key"));
    });
    renderExpr();
  }

  /* ----- Live preview: clipboard filter ----- */
  function wireClipboard() {
    var view = document.getElementById("view-clip");
    if (!view) return;
    var search = view.querySelector("[data-clip-search]");
    var filters = view.querySelector("[data-clip-filters]");
    var list = view.querySelector("[data-clip-list]");
    var empty = view.querySelector("[data-clip-empty]");
    var items = Array.prototype.slice.call(list.querySelectorAll(".clip-item"));
    var kind = "all";

    function apply() {
      var q = (search.value || "").trim().toLowerCase();
      var shown = 0;
      items.forEach(function (it) {
        var okKind = kind === "all" || it.getAttribute("data-kind") === kind;
        var okText = !q || (it.getAttribute("data-text") || "").toLowerCase().indexOf(q) !== -1;
        var show = okKind && okText;
        it.hidden = !show;
        if (show) shown++;
      });
      if (empty) empty.hidden = shown !== 0;
    }
    search.addEventListener("input", apply);
    filters.addEventListener("click", function (e) {
      var b = e.target.closest("button[data-filter]");
      if (!b) return;
      kind = b.getAttribute("data-filter");
      filters.querySelectorAll("button").forEach(function (x) {
        x.setAttribute("aria-pressed", String(x === b));
      });
      apply();
    });
    apply();
  }

  /* ----- Live preview: unit converter ----- */
  function wireConverter() {
    var view = document.getElementById("view-conv");
    if (!view) return;
    var cats = view.querySelector("[data-conv-cats]");
    var fromSel = view.querySelector("[data-conv-from]");
    var toSel = view.querySelector("[data-conv-to]");
    var valIn = view.querySelector("[data-conv-value]");
    var swap = view.querySelector("[data-conv-swap]");
    var out = view.querySelector("[data-conv-out]");
    var rate = view.querySelector("[data-conv-rate]");

    var DATA = {
      length: {
        name: "metre",
        units: { m: ["metres", 1], cm: ["centimetres", 0.01], mm: ["millimetres", 0.001], km: ["kilometres", 1000], mi: ["miles", 1609.344], yd: ["yards", 0.9144], ft: ["feet", 0.3048], in: ["inches", 0.0254] },
        order: ["m", "cm", "mm", "km", "mi", "yd", "ft", "in"]
      },
      weight: {
        name: "kilogram",
        units: { kg: ["kilograms", 1], g: ["grams", 0.001], mg: ["milligrams", 1e-6], lb: ["pounds", 0.45359237], oz: ["ounces", 0.028349523], st: ["stone", 6.35029318] },
        order: ["kg", "g", "mg", "lb", "oz", "st"]
      },
      temp: {
        name: "degree",
        units: { C: ["°C", 0], F: ["°F", 0], K: ["K", 0] },
        order: ["C", "F", "K"]
      }
    };
    var cat = "length";

    function fillSelect(sel, catKey, chosen) {
      sel.innerHTML = "";
      DATA[catKey].order.forEach(function (u) {
        var o = document.createElement("option");
        o.value = u; o.textContent = u + " — " + DATA[catKey].units[u][0];
        if (u === chosen) o.selected = true;
        sel.appendChild(o);
      });
    }
    function tempTo(base, u) { // base is Celsius
      if (u === "C") return base;
      if (u === "F") return base * 9 / 5 + 32;
      return base + 273.15;
    }
    function tempFrom(v, u) { // to Celsius
      if (u === "C") return v;
      if (u === "F") return (v - 32) * 5 / 9;
      return v - 273.15;
    }
    function round(n) { return Math.round((n + Number.EPSILON) * 1e6) / 1e6; }

    function compute() {
      var v = parseFloat(valIn.value);
      if (isNaN(v)) { out.textContent = "Enter a number"; rate.textContent = ""; return; }
      var fu = fromSel.value, tu = toSel.value, res, perText;
      if (cat === "temp") {
        res = round(tempTo(tempFrom(v, fu), tu));
        perText = "Converted on your device.";
      } else {
        var f = DATA[cat].units[fu][1], t = DATA[cat].units[tu][1];
        res = round(v * f / t);
        var per = round(1 * f / t);
        perText = "1 " + fu + " = " + per + " " + tu + " · computed on your device.";
      }
      var fuName = DATA[cat].units[fu][0], tuName = DATA[cat].units[tu][0];
      out.textContent = v + " " + fuName + " = " + res + " " + tuName;
      rate.textContent = perText;
    }

    cats.addEventListener("click", function (e) {
      var b = e.target.closest("button[data-cat]");
      if (!b) return;
      cat = b.getAttribute("data-cat");
      cats.querySelectorAll("button").forEach(function (x) { x.setAttribute("aria-pressed", String(x === b)); });
      var d = DATA[cat].order;
      fillSelect(fromSel, cat, d[0]);
      fillSelect(toSel, cat, d[1] || d[0]);
      compute();
    });
    swap.addEventListener("click", function () {
      var a = fromSel.value; fromSel.value = toSel.value; toSel.value = a; compute();
    });
    [valIn, fromSel, toSel].forEach(function (el) {
      el.addEventListener("input", compute);
      el.addEventListener("change", compute);
    });

    // initial
    fillSelect(fromSel, "length", "m");
    fillSelect(toSel, "length", "cm");
    compute();
  }

  /* ----- Docs: table-of-contents scroll-spy ----- */
  function wireDocsToc() {
    var toc = document.querySelector(".docs-toc");
    var main = document.querySelector(".docs-main");
    if (!toc || !main) return;
    var links = Array.prototype.slice.call(toc.querySelectorAll("a[href^='#']"));
    if (!links.length) return;
    var map = {};
    links.forEach(function (a) {
      var id = a.getAttribute("href").slice(1);
      var sec = document.getElementById(id);
      if (sec) map[id] = a;
    });
    function setActive(id) {
      links.forEach(function (a) { a.classList.toggle("active", a.getAttribute("href") === "#" + id); });
    }
    if ("IntersectionObserver" in window) {
      var io = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) {
          if (en.isIntersecting) setActive(en.target.id);
        });
      }, { rootMargin: "-20% 0px -70% 0px", threshold: 0 });
      Object.keys(map).forEach(function (id) {
        var sec = document.getElementById(id);
        if (sec) io.observe(sec);
      });
    }
  }

  /* ----- boot ----- */
  function init() {
    wireThemeToggle();
    wireNav();
    wireLiveTabs();
    wireTimer();
    wireClocks();
    wireCalc();
    wireClipboard();
    wireConverter();
    wireDocsToc();
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
