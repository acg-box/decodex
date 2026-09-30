function appcastRenderPrimary(root, latest, appName) {
  const primary = root.querySelector("[data-appcast-primary]");
  const meta = root.querySelector("[data-appcast-primary-meta]");
  if (!(primary instanceof HTMLAnchorElement) || !(meta instanceof HTMLElement)) return;

  primary.href = latest.url;
  primary.setAttribute("aria-label", `Download ${appName} ${latest.shortVersion}`);
  const dateLabel = appcastFormatDate(latest.pubDate);
  meta.textContent =
    dateLabel.length > 0 ? `${latest.shortVersion} · ${dateLabel}` : latest.shortVersion;
}

function appcastRenderList(root, items, appName) {
  const list = root.querySelector("[data-appcast-list]");
  const hoverzone = root.querySelector("[data-appcast-hoverzone]");
  if (!(list instanceof HTMLElement) || !(hoverzone instanceof HTMLElement)) return;

  if (items.length === 0) {
    list.innerHTML = '<li class="appcast-download__empty">No builds listed.</li>';
    hoverzone.dataset.appcastDisabled = "true";
    return;
  }

  hoverzone.dataset.appcastDisabled = "false";
  const entries = document.createDocumentFragment();
  for (const item of items) {
    const dateLabel = appcastFormatDate(item.pubDate);
    const entry = document.createElement("li");
    entry.className = "appcast-download__item";
    entry.setAttribute("role", "none");

    const link = document.createElement("a");
    link.className = "appcast-download__version";
    link.setAttribute("role", "menuitem");
    link.href = item.url;
    link.target = "_blank";
    link.rel = "noreferrer";
    link.setAttribute("aria-label", `Download ${appName} ${item.shortVersion}`);

    const title = document.createElement("span");
    title.className = "appcast-download__version-title";
    title.textContent = item.shortVersion;
    const meta = document.createElement("span");
    meta.className = "appcast-download__version-meta";
    meta.textContent = dateLabel || "Date unavailable";
    link.append(title, meta);
    entry.append(link);
    entries.append(entry);
  }
  list.replaceChildren(entries);
}

function appcastRenderFailure(root) {
  const meta = root.querySelector("[data-appcast-primary-meta]");
  const list = root.querySelector("[data-appcast-list]");
  const hoverzone = root.querySelector("[data-appcast-hoverzone]");
  if (meta instanceof HTMLElement) meta.textContent = "Latest unavailable";
  if (list instanceof HTMLElement) {
    list.innerHTML = '<li class="appcast-download__empty">Version list unavailable.</li>';
  }
  if (hoverzone instanceof HTMLElement) hoverzone.dataset.appcastDisabled = "true";
}
