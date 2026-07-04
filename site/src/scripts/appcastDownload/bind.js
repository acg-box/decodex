async function appcastApply(root) {
  const url = root.dataset.appcastUrl;
  const appName = root.dataset.appName || "Codex (Beta).app";
  if (!url) return;

  try {
    const items = await appcastFetchOnce(url);
    if (items.length === 0) throw new Error("appcast empty");
    appcastRenderPrimary(root, items[0], appName);
    appcastRenderList(root, items, appName);
  } catch {
    appcastRenderFailure(root);
  }
}

function bindAppcastDownloadWidget() {
  document.querySelectorAll("[data-appcast-root]").forEach((root) => {
    if (!(root instanceof HTMLElement)) return;
    if (root.dataset.appcastLoaded === "true") return;
    root.dataset.appcastLoaded = "true";
    appcastAttachHover(root);
    void appcastApply(root);
  });
}

document.addEventListener("astro:page-load", bindAppcastDownloadWidget);
document.addEventListener("DOMContentLoaded", bindAppcastDownloadWidget);
bindAppcastDownloadWidget();
