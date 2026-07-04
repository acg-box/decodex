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
