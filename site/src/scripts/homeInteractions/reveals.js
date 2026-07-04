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
