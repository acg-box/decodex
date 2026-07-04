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
  list.innerHTML = items
    .map((item) => {
      const dateLabel = appcastFormatDate(item.pubDate);
      const meta = dateLabel.length > 0 ? dateLabel : "Date unavailable";
      return `
          <li class="appcast-download__item" role="none">
            <a class="appcast-download__version" role="menuitem" href="${item.url}" target="_blank" rel="noreferrer" aria-label="Download ${appName} ${item.shortVersion}">
              <span class="appcast-download__version-title">${item.shortVersion}</span>
              <span class="appcast-download__version-meta">${meta}</span>
            </a>
          </li>
        `;
    })
    .join("");
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
