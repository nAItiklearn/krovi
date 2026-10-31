/* Krovi — minimal vanilla JS. No external dependencies, no trackers.
   1. Theme toggle (light / dark) persisted in localStorage (try/catch).
   2. Mobile nav open/close. */
(function () {
  "use strict";

  var root = document.documentElement;
  var STORAGE_KEY = "krovi-theme";

  /* ----- Theme ----- */
  function storedTheme() {
    try {
      return window.localStorage.getItem(STORAGE_KEY);
    } catch (e) {
      return null;
    }
  }

  function saveTheme(value) {
    try {
      window.localStorage.setItem(STORAGE_KEY, value);
    } catch (e) {
      /* private mode / blocked storage: toggle still works for this page */
    }
  }

  // Apply any saved preference immediately (the inline <head> script also does
  // this to avoid a flash; this is the resilient fallback).
  var saved = storedTheme();
  if (saved === "light" || saved === "dark") {
    root.setAttribute("data-theme", saved);
  }

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

    // Close when a link is chosen or Esc is pressed.
    links.addEventListener("click", function (e) {
      if (e.target.closest("a")) setOpen(false);
    });
    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape") setOpen(false);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () {
      wireThemeToggle();
      wireNav();
    });
  } else {
    wireThemeToggle();
    wireNav();
  }
})();
