(function () {
  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

  function all(selector) {
    return Array.prototype.slice.call(document.querySelectorAll(selector));
  }

  function bindReveals() {
    var targets = all("[data-reveal]");
    if (targets.length === 0) return;

    if (reduceMotion.matches || !("IntersectionObserver" in window)) {
      targets.forEach(function (target) {
        target.classList.add("is-visible");
      });
      return;
    }

    var observer = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          entry.target.classList.add("is-visible");
          observer.unobserve(entry.target);
        });
      },
      { threshold: 0.18 },
    );

    targets.forEach(function (target, index) {
      if (target.dataset.revealBound === "true") return;
      target.dataset.revealBound = "true";
      target.classList.add("reveal-ready");
      target.style.setProperty("--reveal-delay", String(Math.min(index * 55, 220)) + "ms");
      observer.observe(target);
    });
  }

  function bindPressStates() {
    all("a, button").forEach(function (target) {
      if (!(target instanceof HTMLElement) || target.dataset.pressBound === "true") return;
      target.dataset.pressBound = "true";
      var clear = function () {
        target.dataset.pressed = "false";
      };
      target.addEventListener("pointerdown", function () {
        target.dataset.pressed = "true";
      });
      target.addEventListener("pointerup", clear);
      target.addEventListener("pointercancel", clear);
      target.addEventListener("pointerleave", clear);
      target.addEventListener("blur", clear);
      target.addEventListener("keydown", function (event) {
        if (event.key === "Enter" || event.key === " ") target.dataset.pressed = "true";
      });
      target.addEventListener("keyup", clear);
    });
  }

  function bindHeroPointer() {
    var hero = document.querySelector(".official-hero");
    if (!(hero instanceof HTMLElement) || hero.dataset.pointerBound === "true" || reduceMotion.matches) return;
    hero.dataset.pointerBound = "true";
    var frame = 0;
    var nextX = 0;
    var nextY = 0;
    function applyShift() {
      hero.style.setProperty("--hero-shift-x", nextX.toFixed(2) + "px");
      hero.style.setProperty("--hero-shift-y", nextY.toFixed(2) + "px");
      frame = 0;
    }
    hero.addEventListener("pointermove", function (event) {
      var rect = hero.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      nextX = ((event.clientX - rect.left) / rect.width - 0.5) * -10;
      nextY = ((event.clientY - rect.top) / rect.height - 0.5) * -8;
      if (!frame) frame = window.requestAnimationFrame(applyShift);
    });
    hero.addEventListener("pointerleave", function () {
      nextX = 0;
      nextY = 0;
      if (!frame) frame = window.requestAnimationFrame(applyShift);
    });
  }

  function bind() {
    bindReveals();
    bindPressStates();
    bindHeroPointer();
  }

  document.addEventListener("astro:page-load", bind);
  document.addEventListener("DOMContentLoaded", bind);
  bind();
})();
