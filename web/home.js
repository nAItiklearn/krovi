/* Krovi website — home page interactions.
   laptop stage (lazy, scaled live demo) · simulated live clipboard / media / stats ·
   paste stack · pomodoro · calendar · tool explorer · voice demo · command palette ·
   privacy request counter · world clocks.
   Everything is local simulation or DOM work: this file makes no network requests. */
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
  function rand(a, b) { return a + Math.random() * (b - a); }
  function clamp(v, a, b) { return Math.max(a, Math.min(b, v)); }
  function pad(n) { return n < 10 ? "0" + n : String(n); }
  function mmss(s) { s = Math.max(0, Math.round(s)); return pad(Math.floor(s / 60)) + ":" + pad(s % 60); }

  /** Run `fn` on an interval only while `node` is on screen and the tab is visible. */
  function whileVisible(node, fn, ms) {
    var on = false, timer = null;
    function start() { if (!timer) timer = setInterval(function () { if (!document.hidden) fn(); }, ms); }
    function stop() { clearInterval(timer); timer = null; }
    if (!("IntersectionObserver" in window)) { start(); return; }
    new IntersectionObserver(function (entries) {
      on = entries[0].isIntersecting;
      if (on) start(); else stop();
    }, { threshold: 0.1 }).observe(node);
  }

  /* ================= laptop stage: lazy iframe, scaled to fit ================= */
  function initStage() {
    var stage = $("[data-stage]");
    if (!stage) return;
    var screen = $("[data-screen]", stage);
    var frame = $("[data-demo-frame]", stage);
    var load = $(".pz-load", stage);
    var small = window.matchMedia("(max-width: 820px)");
    var started = false;

    function fit() { screen.style.setProperty("--s", (screen.clientWidth / 1152).toFixed(5)); }
    fit();
    if ("ResizeObserver" in window) new ResizeObserver(fit).observe(screen);
    else window.addEventListener("resize", fit);

    function ready() { stage.classList.add("is-ready"); }
    function fail() {
      if (load) load.textContent = "The demo couldn’t load here. Open it in its own page →";
      if (load) { var a = el("a", "", ""); a.href = "/demo/"; a.className = "sr-only"; a.textContent = "Open the live demo"; load.appendChild(a); }
    }
    function onLoad() {
      var ok = false;
      try { ok = /krovi/i.test(frame.contentDocument.title || ""); } catch (e) { ok = true; }
      if (!ok) { fail(); return; }
      if (window.krovi && window.krovi.syncDemoFrames) window.krovi.syncDemoFrames();
      setTimeout(ready, 180);
    }
    function start() {
      if (started || small.matches) return;
      started = true;
      frame.addEventListener("load", onLoad);
      frame.src = frame.getAttribute("data-src");
    }
    function whenIdle(fn) {
      var go = function () { if ("requestIdleCallback" in window) requestIdleCallback(fn, { timeout: 1500 }); else setTimeout(fn, 200); };
      if (document.readyState === "complete") go(); else window.addEventListener("load", go, { once: true });
    }
    if ("IntersectionObserver" in window) {
      var io = new IntersectionObserver(function (entries) {
        if (entries[0].isIntersecting) { io.disconnect(); whenIdle(start); }
      }, { rootMargin: "300px 0px" });
      io.observe(stage);
    } else {
      whenIdle(start);
    }
    small.addEventListener && small.addEventListener("change", function () { if (!small.matches) whenIdle(start); });
  }

  /* ================= live clipboard (simulated) ================= */
  function initClipboardSim() {
    var card = $("[data-live-clip]");
    if (!card) return;
    var list = $("[data-clip-list]", card);
    var pill = $("[data-clip-pill]", card);
    var count = $("[data-clip-count]", card);
    var total = 4;
    var samples = {
      link: ["krovi.tech/compare", "github.com/nAItiklearn/krovi/releases", "open-meteo.com/en/docs"],
      text: ["Meeting moved to 4:30", "Rotate the API keys on Friday", "Draft: release notes for 0.2", "Order #88213 has shipped"],
      image: ["screenshot-1433.png", "mockup-final.png", "receipt-october.jpg"]
    };
    var labels = { link: "Link", text: "Text", image: "Image" };
    var seq = { link: 0, text: 0, image: 0 };

    function add(kind) {
      var pool = samples[kind];
      var text = pool[seq[kind]++ % pool.length];
      $$(".clip-time", list).forEach(function (t) {
        if (t.textContent === "now") t.textContent = "1m";
        else if (t.textContent === "1m") t.textContent = "3m";
      });
      var li = el("li", "clip-row is-new");
      li.appendChild(el("span", "kind", labels[kind]));
      li.appendChild(el("span", "clip-text", text));
      li.appendChild(el("span", "clip-time", "now"));
      var pinned = $$(".clip-row", list).filter(function (r) { return r.querySelector("svg"); }).length;
      var first = $$(".clip-row", list)[pinned] || null;
      list.insertBefore(li, first || list.firstChild);
      var rows = $$(".clip-row", list);
      while (rows.length > 6) { list.removeChild(rows[rows.length - 1]); rows.pop(); }
      total++;
      count.textContent = total + " clips";
      pill.classList.remove("glow");
      void pill.offsetWidth;
      pill.classList.add("glow");
    }
    $$("[data-copy-kind]", card).forEach(function (b) {
      b.addEventListener("click", function () { add(b.getAttribute("data-copy-kind")); });
    });
    if (!reduceMotion) {
      var kinds = ["text", "link", "text", "image"], k = 0;
      whileVisible(card, function () { add(kinds[k++ % kinds.length]); }, 7500);
    }
  }

  /* ================= now playing (simulated) ================= */
  function initMediaSim() {
    var player = $("[data-player]");
    if (!player) return;
    var tracks = [
      { t: "Low Tide", a: "Ana Ferreira", d: 200, c: "", s: "Browser" },
      { t: "Paper Lanterns", a: "The Kestrels", d: 187, c: "t2", s: "Music app" },
      { t: "Night Bus", a: "Mira Das", d: 243, c: "t3", s: "Browser" }
    ];
    var i = 0, pos = 42, playing = true;
    var title = $("[data-track-title]", player), artist = $("[data-track-artist]", player), src = $("[data-track-src]", player);
    var art = $("[data-art]", player), prog = $("[data-prog]", player), posEl = $("[data-pos]", player), durEl = $("[data-dur]", player);
    prog.className = "";
    function fmt(s) { return Math.floor(s / 60) + ":" + pad(Math.floor(s % 60)); }
    function render() {
      var t = tracks[i];
      title.textContent = t.t; artist.textContent = t.a; src.textContent = t.s;
      art.className = "art" + (t.c ? " " + t.c : "");
      posEl.textContent = fmt(pos); durEl.textContent = fmt(t.d);
      prog.style.width = (pos / t.d * 100).toFixed(2) + "%";
      player.setAttribute("data-playing", String(playing));
    }
    function go(d) { i = (i + d + tracks.length) % tracks.length; pos = 0; render(); }
    $$("[data-media]", player).forEach(function (b) {
      b.addEventListener("click", function () {
        var m = b.getAttribute("data-media");
        if (m === "toggle") { playing = !playing; render(); }
        else go(m === "next" ? 1 : -1);
      });
    });
    whileVisible(player, function () {
      if (!playing) return;
      pos += 0.5;
      if (pos >= tracks[i].d) go(1); else render();
    }, 500);
    render();
  }

  /* ================= RAM / disk strip (simulated) ================= */
  function initStatsSim() {
    var box = $("[data-stats]");
    if (!box) return;
    var mem = $("[data-mem]", box), memL = $("[data-mem-label]", box), disk = $("[data-disk]", box), diskL = $("[data-disk-label]", box), cpuL = $("[data-cpu-label]", box);
    var line = $("[data-cpu-line]", box), area = $("[data-cpu-area]", box);
    mem.className = ""; disk.className = "";
    var memTotal = 16, memUsed = 9.8, diskTotal = 953, diskUsed = 412, cpu = [], N = 30;
    for (var k = 0; k < N; k++) cpu.push(rand(8, 22));
    function render() {
      mem.style.width = (memUsed / memTotal * 100).toFixed(1) + "%";
      memL.textContent = memUsed.toFixed(1) + " / " + memTotal + " GB";
      disk.style.width = (diskUsed / diskTotal * 100).toFixed(1) + "%";
      diskL.textContent = Math.round(diskUsed) + " / " + diskTotal + " GB";
      cpuL.textContent = Math.round(cpu[N - 1]) + "%";
      var pts = cpu.map(function (v, idx) { return [idx * 200 / (N - 1), 42 - clamp(v, 0, 100) / 100 * 38]; });
      var d = pts.map(function (p, idx) { return (idx ? "L" : "M") + p[0].toFixed(1) + " " + p[1].toFixed(1); }).join(" ");
      line.setAttribute("d", d);
      area.setAttribute("d", d + " L200 44 L0 44 Z");
    }
    function tick() {
      memUsed = clamp(memUsed + rand(-0.18, 0.2), 8.4, 12.2);
      diskUsed = clamp(diskUsed + rand(0, 0.06), 412, 420);
      cpu.shift(); cpu.push(clamp(cpu[cpu.length - 1] + rand(-9, 9), 3, 64));
      render();
    }
    render();
    if (!reduceMotion) whileVisible(box, tick, 2000);
  }

  /* ================= paste stack ================= */
  function initPaste() {
    var box = $("[data-paste]");
    if (!box) return;
    var list = $("[data-paste-list]", box), count = $("[data-paste-count]", box), toast = $("[data-paste-toast]", box);
    var snips = ["jane@example.com", "+91 98765 43210", "221B Baker Street, London", "Invoice #2041", "Thanks, speak soon!", "https://krovi.tech", "ORD-88213", "Sort code 04-00-04", "Meeting link: meet.example/standup", "Q3 budget.xlsx"];
    var stack = [], next = 0, lastPushed = false;
    function seed() { stack = []; next = 0; push(true); push(true); push(true); lastPushed = false; }
    function render(enter) {
      list.textContent = "";
      for (var i = 0; i < 9; i++) {
        var li = el("li", stack[i] ? (i === 0 ? "top" : "") : "empty");
        li.appendChild(el("span", "n", String(i + 1)));
        li.appendChild(el("span", "t", stack[i] || "empty"));
        if (enter && i === 0 && stack[0]) li.classList.add("enter");
        list.appendChild(li);
      }
      count.textContent = stack.length + " / 9";
    }
    function push(silent) {
      var s = snips[next++ % snips.length];
      stack.unshift(s);
      if (stack.length > 9) stack.length = 9;
      lastPushed = true;
      if (!silent) { render(true); toast.innerHTML = "Pushed <b></b> to slot 1."; $("b", toast).textContent = s; }
    }
    function pop() {
      if (!stack.length) { toast.textContent = "The stack is empty. Push something first."; return; }
      var first = list.firstChild;
      var s = stack.shift();
      toast.innerHTML = "Pasted <b></b> and removed it from the stack.";
      $("b", toast).textContent = s;
      if (first && !reduceMotion) { first.classList.add("leave"); setTimeout(function () { render(false); }, 260); }
      else render(false);
    }
    $("[data-paste-push]", box).addEventListener("click", function () { push(false); });
    $("[data-paste-pop]", box).addEventListener("click", pop);
    $("[data-paste-reset]", box).addEventListener("click", function () { seed(); render(false); toast.textContent = "Reset. Push a few snippets, then pop them back in reverse."; });
    seed(); render(false);
  }

  /* ================= pomodoro ================= */
  function initPomodoro() {
    var box = $("[data-pomo]");
    if (!box) return;
    var wrap = $(".pomo", box), time = $("[data-pomo-time]", box), mode = $("[data-pomo-mode]", box), ring = $("[data-pomo-ring]", box);
    var sessLabel = $("[data-pomo-sess]", box), dots = $$("[data-sess]", box);
    var toggle = $("[data-pomo-toggle]", box), reset = $("[data-pomo-reset]", box), fast = $("[data-pomo-fast]", box);
    var C = 628.3, WORK = 1500, BREAK = 300;
    var state = { kind: "work", total: WORK, left: WORK, sess: 1, running: false, speed: 1, timer: null };
    function render() {
      time.textContent = mmss(state.left);
      mode.textContent = state.kind === "work" ? "Work" : "Break";
      wrap.setAttribute("data-mode", state.kind);
      ring.style.strokeDashoffset = (C * (1 - state.left / state.total)).toFixed(2);
      sessLabel.textContent = "Session " + state.sess + " of 4";
      dots.forEach(function (d, i) { d.classList.toggle("done", i < state.sess - 1 || (state.kind === "break" && i === state.sess - 1)); });
      toggle.textContent = state.running ? "Pause" : (state.left === state.total ? "Start" : "Resume");
    }
    function stop() { clearInterval(state.timer); state.timer = null; state.running = false; }
    function finish() {
      if (state.kind === "work") {
        state.kind = "break"; state.total = state.left = BREAK;
      } else {
        if (state.sess >= 4) { stop(); state.kind = "work"; state.sess = 1; state.total = state.left = WORK; render(); mode.textContent = "All done"; return; }
        state.sess++; state.kind = "work"; state.total = state.left = WORK;
      }
      render();
    }
    function tick() {
      state.left -= state.speed;
      if (state.left <= 0) { state.left = 0; render(); finish(); } else render();
    }
    toggle.addEventListener("click", function () {
      if (state.running) { stop(); } else { state.running = true; state.timer = setInterval(tick, 1000); }
      render();
    });
    reset.addEventListener("click", function () { stop(); state.kind = "work"; state.sess = 1; state.total = state.left = WORK; render(); });
    fast.addEventListener("click", function () {
      state.speed = state.speed === 1 ? 30 : 1;
      fast.setAttribute("aria-pressed", String(state.speed === 30));
    });
    render();
  }

  /* ================= calendar (sample fixed-date holidays) ================= */
  function initCalendar() {
    var box = $("[data-calendar]");
    if (!box) return;
    var title = $("[data-cal-title]", box), grid = $("[data-cal-grid]", box), list = $("[data-cal-list]", box);
    var H = {
      "01-26": ["Republic Day", "gazetted"], "04-14": ["Ambedkar Jayanti", "gazetted"], "05-01": ["Labour Day", "observance"],
      "08-15": ["Independence Day", "gazetted"], "09-05": ["Teachers’ Day", "observance"], "10-02": ["Gandhi Jayanti", "gazetted"],
      "10-31": ["National Unity Day", "observance"], "11-14": ["Children’s Day", "observance"], "11-26": ["Constitution Day", "observance"],
      "12-25": ["Christmas Day", "gazetted"]
    };
    var months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    var dows = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    var now = new Date();
    var y = now.getFullYear(), m = now.getMonth();
    function monthHas(mm) { return Object.keys(H).some(function (k) { return parseInt(k.slice(0, 2), 10) === mm + 1; }); }
    for (var guard = 0; guard < 12 && !monthHas(m); guard++) { m = (m + 1) % 12; if (m === 0) y++; }

    function render() {
      title.textContent = months[m] + " " + y;
      grid.textContent = "";
      dows.forEach(function (d) { grid.appendChild(el("span", "dow", d)); });
      var first = new Date(y, m, 1);
      var lead = (first.getDay() + 6) % 7;
      var days = new Date(y, m + 1, 0).getDate();
      for (var i = 0; i < lead; i++) grid.appendChild(el("span", "day out", ""));
      list.textContent = "";
      var found = 0;
      for (var d = 1; d <= days; d++) {
        var key = pad(m + 1) + "-" + pad(d);
        var c = el("span", "day", String(d));
        var isToday = d === now.getDate() && m === now.getMonth() && y === now.getFullYear();
        if (isToday) c.classList.add("today");
        if (H[key]) {
          c.setAttribute("data-h", H[key][1]);
          c.setAttribute("title", H[key][0]);
          found++;
          var li = el("li");
          li.appendChild(el("span", "tag" + (H[key][1] === "observance" ? " o" : ""), H[key][1] === "gazetted" ? "Gazetted" : "Observance"));
          li.appendChild(el("span", "", H[key][0]));
          li.appendChild(el("span", "d", months[m].slice(0, 3) + " " + d));
          list.appendChild(li);
        }
        grid.appendChild(c);
      }
      if (!found) { var n = el("li", "none", "No sample holidays this month."); list.appendChild(n); }
    }
    $("[data-cal-prev]", box).addEventListener("click", function () { m--; if (m < 0) { m = 11; y--; } render(); });
    $("[data-cal-next]", box).addEventListener("click", function () { m++; if (m > 11) { m = 0; y++; } render(); });
    render();
  }

  /* ================= tool explorer (tabs) ================= */
  function initExplorer() {
    var ex = $("[data-explorer]");
    if (!ex) return;
    var tabs = $$("[role=tab]", ex);
    function select(tab, focus) {
      tabs.forEach(function (t) {
        var on = t === tab;
        t.setAttribute("aria-selected", String(on));
        t.tabIndex = on ? 0 : -1;
        var p = document.getElementById(t.getAttribute("aria-controls"));
        if (p) { p.hidden = !on; p.classList.toggle("on", on); }
      });
      if (focus) tab.focus();
    }
    tabs.forEach(function (tab, i) {
      tab.addEventListener("click", function () { select(tab, false); });
      tab.addEventListener("keydown", function (e) {
        var n = null;
        if (e.key === "ArrowDown" || e.key === "ArrowRight") n = tabs[(i + 1) % tabs.length];
        else if (e.key === "ArrowUp" || e.key === "ArrowLeft") n = tabs[(i - 1 + tabs.length) % tabs.length];
        else if (e.key === "Home") n = tabs[0];
        else if (e.key === "End") n = tabs[tabs.length - 1];
        if (n) { e.preventDefault(); select(n, true); }
      });
    });
    var first = $(".ex-panel.on", ex);
    if (first) first.hidden = false;
  }

  /* ================= voice demo ================= */
  function initVoice() {
    var box = $("[data-voice]");
    if (!box) return;
    var wave = $("[data-wave]", box), status = $("[data-voice-status]", box), text = $("[data-voice-text]", box);
    var result = $("[data-voice-result]", box), resultText = $("[data-voice-result-text]", box), btn = $("[data-voice-next]", box);
    for (var w = 0; w < 28; w++) wave.appendChild(document.createElement("i"));
    // only phrasings from the app's closed command list (docs/SPEC.md 11.13); results are simulated
    var DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"], MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    function todayLabel() { var d = new Date(); return DAYS[d.getDay()] + " " + d.getDate() + " " + MONTHS[d.getMonth()]; }
    var script = [
      { mode: "command", say: "open the calendar", res: "Opened Calendar" },
      { mode: "command", say: "start a timer for five minutes", res: "Timer started · 5:00" },
      { mode: "command", say: "set a reminder for five p m to call mom", res: function () { return "Reminder set for today, " + todayLabel() + ", 5:00 PM: Call mom"; } },
      { mode: "note", say: "call Priya about the invoice at four", res: "Saved to Notes: “Call Priya about the invoice at four”" },
      { mode: "command", say: "open downloads", res: "Opened your Downloads folder in Explorer" },
      { mode: "command", say: "launch notepad", res: "Launched Notepad" },
      { mode: "command", say: "close", res: "Folded back to the pill" }
    ];
    var idx = -1, token = 0, auto = true, visible = false, running = false;
    function wait(ms) { return new Promise(function (r) { setTimeout(r, reduceMotion ? Math.min(ms, 120) : ms); }); }
    function setState(s, label) { box.setAttribute("data-state", s); status.textContent = label; }
    async function run() {
      var me = ++token;
      running = true;
      idx = (idx + 1) % script.length;
      var step = script[idx];
      box.setAttribute("data-mode", step.mode);
      result.classList.remove("on");
      text.textContent = "";
      setState("listening", "Listening…");
      await wait(700); if (me !== token) return;
      var span = el("span"); text.appendChild(span);
      var caret = el("span", "caret"); text.appendChild(caret);
      for (var i = 1; i <= step.say.length; i++) {
        span.textContent = step.say.slice(0, i);
        await wait(55); if (me !== token) return;
      }
      await wait(450); if (me !== token) return;
      setState("processing", "Processing…");
      caret.remove();
      await wait(650); if (me !== token) return;
      setState("idle", "Done");
      resultText.textContent = typeof step.res === "function" ? step.res() : step.res;
      result.classList.add("on");
      await wait(3600); if (me !== token) return;
      running = false;
      if (auto && visible && !reduceMotion) run();
    }
    btn.addEventListener("click", function () { auto = true; run(); });
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (entries) {
        visible = entries[0].isIntersecting;
        if (visible && !running && !reduceMotion) run();
      }, { threshold: 0.4 }).observe(box);
    }
  }

  /* ================= command palette (simulation) ================= */
  function initPalette() {
    var box = $("[data-palette]");
    if (!box) return;
    var input = $("[data-pal-input]", box), items = $$(".pal-item", box), empty = $("[data-pal-empty]", box), res = $("[data-pal-res]", box);
    var sel = 0;
    items.forEach(function (it, i) { it.id = "pal-opt-" + i; });
    function visibleItems() { return items.filter(function (i) { return !i.hidden; }); }
    function select(i) {
      var v = visibleItems();
      if (!v.length) { input.removeAttribute("aria-activedescendant"); return; }
      sel = (i + v.length) % v.length;
      items.forEach(function (it) { it.setAttribute("aria-selected", "false"); });
      v[sel].setAttribute("aria-selected", "true");
      input.setAttribute("aria-activedescendant", v[sel].id);
      // Keep the selection visible inside the palette list ONLY. scrollIntoView() would
      // also scroll the page (it scrolls every ancestor), which on load yanked visitors
      // down to the Keyboard-first section.
      var it = v[sel], list = it.parentElement;
      if (list && list.scrollHeight > list.clientHeight) {
        var top = it.offsetTop - list.offsetTop, bottom = top + it.offsetHeight;
        if (top < list.scrollTop) list.scrollTop = top;
        else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
      }
    }
    function filter() {
      var q = input.value.trim().toLowerCase();
      items.forEach(function (it) {
        var hay = (it.getAttribute("data-name") + " " + it.getAttribute("data-hint")).toLowerCase();
        var ok = !q || q.split(/\s+/).every(function (w) { return hay.indexOf(w) !== -1; });
        it.hidden = !ok;
      });
      empty.hidden = visibleItems().length !== 0;
      select(0);
    }
    function open(it) { res.textContent = "→ would open " + it.getAttribute("data-name"); }
    input.addEventListener("input", filter);
    input.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown") { e.preventDefault(); select(sel + 1); }
      else if (e.key === "ArrowUp") { e.preventDefault(); select(sel - 1); }
      else if (e.key === "Enter") { var v = visibleItems(); if (v[sel]) { e.preventDefault(); open(v[sel]); } }
      else if (e.key === "Escape") { input.value = ""; filter(); }
    });
    items.forEach(function (it) {
      it.addEventListener("click", function () { open(it); });
      it.addEventListener("mousemove", function () {
        var v = visibleItems(); var i = v.indexOf(it);
        if (i !== sel && i !== -1) select(i);
      });
    });
    select(0);
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
    if (document.readyState === "complete") measure(); else window.addEventListener("load", function () { setTimeout(measure, 300); });
  }

  /* ================= world clocks ================= */
  function initClocks() {
    var nodes = $$("[data-tz]");
    if (!nodes.length) return;
    function update() {
      nodes.forEach(function (n) {
        try {
          n.textContent = new Intl.DateTimeFormat("en-GB", { timeZone: n.getAttribute("data-tz"), hour: "2-digit", minute: "2-digit", hour12: false }).format(new Date());
        } catch (e) { /* keep placeholder */ }
      });
    }
    update();
    setInterval(update, 15000);
  }

  /* ================= tool chain: seamless drifting tiles =================
     The front lane is authored in HTML. Here we (1) build the other lanes from it in a rotated
     order, (2) append a second copy of every track so translateX(-50% -> 0) loops with no seam,
     and (3) only run the animation while the card is on screen and the tab is visible. */
  function initChain() {
    var card = $("[data-chain]");
    if (!card) return;
    var lanes = $$("[data-lane]", card);
    var front = lanes.filter(function (l) { return l.classList.contains("lane-front"); })[0];
    if (!front) return;
    var base = $$(".tile", front);
    if (!base.length) return;
    lanes.forEach(function (lane) {
      var track = $(".track", lane);
      if (!track) return;
      if (!track.children.length) {
        var shift = parseInt(lane.getAttribute("data-shift") || "0", 10) % base.length;
        for (var i = 0; i < base.length; i++) track.appendChild(base[(i + shift) % base.length].cloneNode(true));
      }
      if (reduceMotion) return; // static pose: one set, no loop
      var first = Array.prototype.slice.call(track.children);
      first.forEach(function (n) {
        var c = n.cloneNode(true);
        c.setAttribute("aria-hidden", "true");
        track.appendChild(c);
      });
    });
    if (reduceMotion || !("IntersectionObserver" in window)) return;
    card.classList.add("is-live");
    var inView = false;
    function sync() { card.classList.toggle("is-on", inView && !document.hidden); }
    new IntersectionObserver(function (entries) { inView = entries[0].isIntersecting; sync(); }, { rootMargin: "80px 0px" }).observe(card);
    document.addEventListener("visibilitychange", sync);
  }

  /* ================= inline clips: lazy, muted, looped, only while visible =================
     preload="none" + a poster means nothing downloads until the clip is on screen. Reduced motion
     or Data Saver: the poster stays, nothing plays. A pause button covers WCAG 2.2.2. */
  function initClips() {
    var figs = $$("[data-clip]");
    if (!figs.length) return;
    var saver = !!(navigator.connection && navigator.connection.saveData);
    figs.forEach(function (fig) {
      var v = $("video", fig), btn = $(".clip-toggle", fig);
      if (!v) return;
      if (reduceMotion || saver || !("IntersectionObserver" in window)) { fig.classList.add("is-static"); return; }
      var inView = false, userPaused = false;
      function play() { var p = v.play(); if (p && p.catch) p.catch(function () {}); }
      function sync() { if (inView && !userPaused && !document.hidden) play(); else v.pause(); }
      new IntersectionObserver(function (entries) { inView = entries[0].isIntersecting; sync(); }, { threshold: 0.3 }).observe(fig);
      document.addEventListener("visibilitychange", sync);
      if (btn) btn.addEventListener("click", function () {
        userPaused = !userPaused;
        btn.setAttribute("aria-pressed", userPaused ? "true" : "false");
        btn.setAttribute("aria-label", userPaused ? "Play this clip" : "Pause this clip");
        sync();
      });
    });
  }

  function init() {
    initChain();
    initClips();
    initStage();
    initClipboardSim();
    initMediaSim();
    initStatsSim();
    initPaste();
    initPomodoro();
    initCalendar();
    initExplorer();
    initVoice();
    initPalette();
    initNetCheck();
    initClocks();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
