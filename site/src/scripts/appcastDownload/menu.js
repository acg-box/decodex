function appcastSetOpenState(root, isOpen) {
  root.dataset.appcastOpen = isOpen ? "true" : "false";
  const primary = root.querySelector("[data-appcast-primary]");
  if (primary instanceof HTMLElement) {
    primary.setAttribute("aria-expanded", isOpen ? "true" : "false");
  }
}

function appcastCancelClose(root) {
  const closeTimer = appcastCloseTimers.get(root);
  if (typeof closeTimer === "number") {
    window.clearTimeout(closeTimer);
    appcastCloseTimers.delete(root);
  }
}

function appcastScheduleClose(root) {
  appcastCancelClose(root);
  const closeTimer = window.setTimeout(() => {
    appcastSetOpenState(root, false);
    appcastCloseTimers.delete(root);
  }, appcastCloseDelayMs);
  appcastCloseTimers.set(root, closeTimer);
}

function appcastAttachHover(root) {
  const hoverzone = root.querySelector("[data-appcast-hoverzone]");
  if (!(hoverzone instanceof HTMLElement)) return;

  hoverzone.addEventListener("pointerenter", () => {
    if (hoverzone.dataset.appcastDisabled === "true") return;
    appcastCancelClose(root);
    appcastSetOpenState(root, true);
  });
  hoverzone.addEventListener("pointerleave", () => {
    appcastScheduleClose(root);
  });
  hoverzone.addEventListener("focusin", () => {
    if (hoverzone.dataset.appcastDisabled === "true") return;
    appcastCancelClose(root);
    appcastSetOpenState(root, true);
  });
  hoverzone.addEventListener("focusout", (event) => {
    const nextTarget = event.relatedTarget;
    if (nextTarget instanceof Node && hoverzone.contains(nextTarget)) return;
    appcastScheduleClose(root);
  });
}
