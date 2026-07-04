function bindHomeInteractions() {
  bindReveals();
  bindPressStates();
  bindHeroPointer();
}

document.addEventListener("astro:page-load", bindHomeInteractions);
document.addEventListener("DOMContentLoaded", bindHomeInteractions);
bindHomeInteractions();
