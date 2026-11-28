/* Krovi website: hero entrance flag, mobile menu sheet and copy buttons.
   No network requests, no storage, no third-party code. */
(function () {
  "use strict";

  document.documentElement.classList.add("m-enter");

  var toggle = document.querySelector(".nav__toggle");
  var sheet = document.querySelector(".nav__sheet");
  if (toggle && sheet) {
    var setOpen = function (open) {
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
      if (open) {
        sheet.setAttribute("data-open", "true");
        sheet.removeAttribute("hidden");
      } else {
        sheet.removeAttribute("data-open");
        sheet.setAttribute("hidden", "");
      }
    };
    toggle.addEventListener("click", function () {
      var isOpen = toggle.getAttribute("aria-expanded") === "true";
      setOpen(!isOpen);
    });
    sheet.addEventListener("click", function (event) {
      if (event.target.closest("a")) setOpen(false);
    });
    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && toggle.getAttribute("aria-expanded") === "true") {
        setOpen(false);
        toggle.focus();
      }
    });
  }

  function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise(function (resolve, reject) {
      var area = document.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.top = "-1000px";
      document.body.appendChild(area);
      area.select();
      try {
        document.execCommand("copy");
        resolve();
      } catch (error) {
        reject(error);
      } finally {
        area.remove();
      }
    });
  }

  function showCopied(button) {
    var label = button.querySelector("span") || button;
    var original = label.textContent;
    button.setAttribute("data-copied", "true");
    label.textContent = "Copied";
    window.setTimeout(function () {
      label.textContent = original;
      button.removeAttribute("data-copied");
    }, 1800);
  }

  document.addEventListener("click", function (event) {
    var button = event.target.closest(".copy-btn");
    if (!button) return;

    var text = button.getAttribute("data-copy");
    if (!text) {
      var block = button.closest(".codeblock");
      var code = block && block.querySelector("pre code");
      if (!code) return;
      text = code.textContent;
    }

    copyText(text).then(function () {
      showCopied(button);
    }).catch(function () {
      /* Leave button label unchanged if clipboard access is denied. */
    });
  });
})();
