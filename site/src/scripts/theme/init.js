(function () {
  var storageKey = "decodex-theme";
  var lightColor = "#f7f8fc";
  var darkColor = "#1a1b26";
  var query = window.matchMedia("(prefers-color-scheme: dark)");

  function readStoredTheme() {
    try {
      var value = localStorage.getItem(storageKey);
      return value === "light" || value === "dark" ? value : "system";
    } catch (_) {
      return "system";
    }
  }

  function resolvedTheme(theme) {
    return theme === "system" ? (query.matches ? "dark" : "light") : theme;
  }

  function applyTheme(theme) {
    var root = document.documentElement;
    var resolved = resolvedTheme(theme);
    root.dataset.theme = theme;
    root.style.colorScheme = resolved;
    var meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute("content", resolved === "dark" ? darkColor : lightColor);
  }

  window.__decodexTheme = {
    get: readStoredTheme,
    set: function (theme) {
      try {
        if (theme === "system") localStorage.removeItem(storageKey);
        else localStorage.setItem(storageKey, theme);
      } catch (_) {}
      applyTheme(theme);
      window.dispatchEvent(new CustomEvent("decodex-theme-change", { detail: { theme: theme } }));
    },
  };

  query.addEventListener("change", function () {
    if (readStoredTheme() === "system") {
      applyTheme("system");
      window.dispatchEvent(new CustomEvent("decodex-theme-change", { detail: { theme: "system" } }));
    }
  });

  applyTheme(readStoredTheme());
})();
