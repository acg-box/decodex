(function () {
  function readTheme() {
    return window.__decodexTheme && typeof window.__decodexTheme.get === "function"
      ? window.__decodexTheme.get()
      : "system";
  }

  function syncButtons(theme) {
    document.querySelectorAll("[data-theme-option]").forEach(function (button) {
      button.setAttribute(
        "aria-pressed",
        button.getAttribute("data-theme-option") === theme ? "true" : "false"
      );
    });
  }

  function bind() {
    document.querySelectorAll("[data-theme-option]").forEach(function (button) {
      if (button.dataset.themeBound === "true") return;
      button.dataset.themeBound = "true";
      button.addEventListener("click", function () {
        var theme = button.getAttribute("data-theme-option");
        if (!theme || !window.__decodexTheme) return;
        window.__decodexTheme.set(theme);
      });
    });
    syncButtons(readTheme());
  }

  document.addEventListener("astro:page-load", bind);
  document.addEventListener("DOMContentLoaded", bind);
  window.addEventListener("decodex-theme-change", function (event) {
    syncButtons(event.detail && event.detail.theme ? event.detail.theme : readTheme());
  });
})();
