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
