// Loaded synchronously in <head> so the theme is set before first paint.
(function () {
  var KEY = "hyper_low-theme";
  var root = document.documentElement;

  function read(key) {
    try { return localStorage.getItem(key); } catch (e) { return null; }
  }

  function write(key, value) {
    try { localStorage.setItem(key, value); } catch (e) {}
  }

  function saved() {
    var t = read(KEY);
    if (t === "light" || t === "dark") return t;
    // Pick up a choice made in the rustdoc API pages (same origin).
    var r = read("rustdoc-theme");
    if (read("rustdoc-use-system-theme") === "false" && (r === "light" || r === "dark")) return r;
    return null;
  }

  function current() {
    return saved() || (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  }

  function label() {
    var btn = document.getElementById("theme-toggle");
    if (!btn) return;
    var dark = root.getAttribute("data-theme") === "dark";
    btn.textContent = dark ? "\u2600 Light" : "\u263E Dark";
    btn.setAttribute("aria-pressed", dark ? "true" : "false");
  }

  function apply(theme) {
    root.setAttribute("data-theme", theme);
    label();
  }

  apply(current());

  document.addEventListener("DOMContentLoaded", function () {
    var btn = document.getElementById("theme-toggle");
    if (btn) {
      btn.addEventListener("click", function () {
        var next = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
        write(KEY, next);
        // Keep the rustdoc API pages in sync.
        write("rustdoc-theme", next);
        write("rustdoc-use-system-theme", "false");
        apply(next);
      });
    }
    label();
  });

  // Other open tabs.
  window.addEventListener("storage", function (e) {
    if (e.key === KEY || e.key === "rustdoc-theme") apply(current());
  });

  // Pages restored from the back/forward cache.
  window.addEventListener("pageshow", function () {
    apply(current());
  });

  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", function () {
    if (!saved()) apply(current());
  });
})();
