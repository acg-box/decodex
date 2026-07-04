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
