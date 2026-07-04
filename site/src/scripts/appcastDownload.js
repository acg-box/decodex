const appcastCloseTimers = new WeakMap();
const appcastCloseDelayMs = 140;

function appcastFormatDate(value) {
  if (typeof value !== "string" || value.length === 0) return "";
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return "";
  return new Intl.DateTimeFormat("en", {
    month: "short",
    day: "numeric",
  }).format(new Date(parsed));
}

function appcastText(node, selector) {
  const target = node.querySelector(selector);
  return target?.textContent?.trim() || "";
}

function appcastParse(xmlText) {
  const doc = new DOMParser().parseFromString(xmlText, "application/xml");
  if (doc.querySelector("parsererror")) {
    throw new Error("appcast XML parse failed");
  }

  const items = Array.from(doc.querySelectorAll("channel > item"))
    .map((item) => {
      const enclosure = item.querySelector("enclosure");
      const url = enclosure?.getAttribute("url") || "";
      const title = appcastText(item, "title");
      const shortVersion =
        item.getElementsByTagNameNS("*", "shortVersionString")[0]?.textContent?.trim() || title;
      const pubDate = appcastText(item, "pubDate");

      return {
        title,
        shortVersion,
        pubDate,
        pubDateMs: Date.parse(pubDate),
        url,
      };
    })
    .filter((item) => item.url.length > 0);

  items.sort((left, right) => {
    const leftMs = Number.isNaN(left.pubDateMs) ? 0 : left.pubDateMs;
    const rightMs = Number.isNaN(right.pubDateMs) ? 0 : right.pubDateMs;
    return rightMs - leftMs;
  });

  return items;
}

async function appcastFetchOnce(url) {
  if (!window.__decodexAppcastPromise) {
    window.__decodexAppcastPromise = fetch(url, { mode: "cors" })
      .then(async (response) => {
        if (!response.ok) {
          throw new Error(`appcast HTTP ${response.status}`);
        }
        return response.text();
      })
      .then(appcastParse);
  }

  return window.__decodexAppcastPromise;
}

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
