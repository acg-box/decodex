// Pixel geometry is local to each part. Bodies provide placement anchors.
const part = (label, rects) => ({ label, rects });
const rect = (x, y, width, height, fill) => [x, y, width, height, fill];

export const bodies = {
  original: {
    label: "Original cloud",
    viewBox: [0, 0, 40, 32],
    firstRow: 7,
    rows: [[16, 6], [14, 8], [13, 12], [13, 12], [13, 15], [13, 15], [11, 18], [10, 19], [9, 21], [9, 23], [9, 23], [9, 23], [10, 22], [11, 20]],
    shade: [[9, 15, 2, 4], [10, 18, 3, 2], [12, 19, 3, 2]],
    highlight: [[20, 7, 2, 2], [23, 9, 2, 2], [26, 11, 2, 2], [28, 13, 2, 2], [30, 15, 2, 4]],
    particles: [[25, 7, 2, 2], [28, 9, 2, 2], [31, 11, 2, 2], [34, 5, 2, 2]],
    anchors: { face: [17, 16], accessory: [12, 11], leftHand: [6, 19], rightHand: [35, 19] },
    reference: "source/original-cloudlets.png",
  },
};

export const palettes = {
  mint: { label: "Mint", base: "#9be9dc", light: "#cefff0", shade: "#50cdbf", ink: "#10121c" },
  lilac: { label: "Lilac", base: "#c9a7ee", light: "#ead7ff", shade: "#a875db", ink: "#181323" },
  sky: { label: "Sky", base: "#98d7f7", light: "#d1f0ff", shade: "#56b6e7", ink: "#111926" },
  peach: { label: "Peach", base: "#ffb5a1", light: "#ffe1cf", shade: "#ee887b", ink: "#201922" },
  butter: { label: "Butter", base: "#ffe994", light: "#fff7c8", shade: "#efc25b", ink: "#211d27" },
  cream: { label: "Cream", base: "#fff0cd", light: "#fff9e8", shade: "#e9cd96", ink: "#24202b" },
  rose: { label: "Rose", base: "#f3a3b9", light: "#ffd8e0", shade: "#d96c92", ink: "#251827" },
  periwinkle: { label: "Periwinkle", base: "#aeb5fc", light: "#e0e3ff", shade: "#7f88e7", ink: "#171c31" },
  ice: { label: "Ice", base: "#9bdbfa", light: "#e0f6ff", shade: "#4caee6", ink: "#172332" },
  sage: { label: "Sage", base: "#a2d6ac", light: "#d8f7d9", shade: "#65a977", ink: "#15291f" },
  coral: { label: "Coral", base: "#ff947e", light: "#ffd0b6", shade: "#e86458", ink: "#281a22" },
  midnight: { label: "Midnight", base: "#384c8f", light: "#8292e5", shade: "#25345f", ink: "#d0f9db" },
};

export const expressions = {
  square: part("Square eyes", [[0, 0, 2, 2, "ink"], [5, 0, 2, 2, "ink"]]),
  sleepy: part("Relaxed eyes", [[0, 1, 2, 1, "ink"], [5, 1, 2, 1, "ink"]]),
  cheerful: part("Happy eyes", [[0, 1, 1, 1, "ink"], [1, 0, 1, 1, "ink"], [2, 1, 1, 1, "ink"], [5, 1, 1, 1, "ink"], [6, 0, 1, 1, "ink"], [7, 1, 1, 1, "ink"]]),
  dot: part("Dot eyes", [[0.5, 0.5, 1, 1.5, "ink"], [5.5, 0.5, 1, 1.5, "ink"]]),
  wink: part("Wink", [[0, 0, 2, 2, "ink"], [5, 1, 1, 1, "ink"], [6, 0, 1, 1, "ink"], [7, 1, 1, 1, "ink"]]),
};

const dark = "#1c2235";
const gold = "#edb947";
const paper = "#fff1cc";
const blue = "#4d7bda";
export const accessories = {
  none: part("None", []),
  headset: part("Side earpiece", [[-1.5, 1.5, 0.5, 2, dark], [-2, 3, 2, 2, dark], [-1.5, 3.5, 0.5, 1, "#343d54"]]),
  "square-glasses": { ...part("Square glasses", [[-1, -1, 4, 0.5, dark], [-1, -1, 0.5, 4, dark], [2.5, -1, 0.5, 4, dark], [-1, 2.5, 4, 0.5, dark], [4, -1, 4, 0.5, dark], [4, -1, 0.5, 4, dark], [7.5, -1, 0.5, 4, dark], [4, 2.5, 4, 0.5, dark], [3, 0.5, 1, 0.5, dark], [-2, 0.5, 1, 0.5, dark], [8, 0.5, 1, 0.5, dark]]), anchor: "face" },
  "round-glasses": { ...part("Round glasses", [[0, -1, 2, 0.5, dark], [-0.5, -0.5, 0.5, 3, dark], [2, -0.5, 0.5, 3, dark], [0, 2.5, 2, 0.5, dark], [5, -1, 2, 0.5, dark], [4.5, -0.5, 0.5, 3, dark], [7, -0.5, 0.5, 3, dark], [5, 2.5, 2, 0.5, dark], [2.5, 0.5, 2, 0.5, dark], [-1.5, 0.5, 1, 0.5, dark], [7.5, 0.5, 1, 0.5, dark]]), anchor: "face" },
  sprout: part("Sprout", [[0, -2, 0.5, 4, "#327b50"], [-2, -2, 2, 1, "#50c37c"], [-2.5, -3, 2, 1, "#7be498"], [0.5, -1.5, 2, 1, "#50c37c"], [1, -2.5, 2, 1, "#7be498"]]),
  "star-pin": part("Star pin", [[0, -1, 1, 3, gold], [-1, 0, 3, 1, gold], [-0.5, 1, 0.5, 0.5, gold], [1, 1, 0.5, 0.5, gold], [0, 0, 1, 0.5, "#fff0a2"]]),
  "navy-cap": part("Navy cap", [[-1, -2, 3, 1, "#2b4b9c"], [-2, -1, 4, 2, "#2b4b9c"], [-2.5, 1, 5.5, 0.5, "#203773"], [0, -0.5, 1, 0.5, "#b9e6ef"]]),
  "leaf-pin": part("Leaf pin", [[0, -1, 1, 3, "#347d54"], [-1.5, -1.5, 1.5, 1, "#68ce8b"], [-2, -2, 1.5, 1, "#96e5a8"], [1, -0.5, 1, 1, "#68ce8b"]]),
  bow: part("Bow", [[-2, -1, 1, 3, "#b194e9"], [-1, -0.5, 1, 2, "#9a79cf"], [0, 0, 1, 1, "#d7b9fc"], [1, -0.5, 1, 2, "#9a79cf"], [2, -1, 1, 3, "#b194e9"]]),
  badge: part("Amber badge", [[-0.5, -1, 1, 3, gold], [-1, -0.5, 2, 2, gold], [-0.5, -0.5, 0.5, 1, "#ffe697"]]),
  barrette: part("Blue barrette", [[-1, -0.5, 3, 1, "#3a9bd5"], [-0.5, -1, 2, 1, "#83d1f5"]]),
  "moon-pin": part("Moon pin", [[0, -1.5, 1, 0.5, "#f5f3e9"], [-1, -1, 1, 3, "#d9dce5"], [0, 1.5, 2, 0.5, "#d9dce5"], [-0.5, 1, 2, 0.5, "#f5f3e9"], [1, 1, 1, 0.5, "#d9dce5"]]),
};

// The prop origin is the grip point; either hand can use every prop.
export const props = {
  none: part("Empty hand", []),
  "blue-book": part("Blue book", [[-2, -5, 4, 4.5, "#304d99"], [-1.5, -4.5, 3, 4, blue], [-1, -4.5, 2.5, 0.5, "#dceeff"], [1, -4, 0.5, 3.5, "#dceeff"]]),
  pencil: part("Pencil", [[-0.5, -5, 1, 4, "#edbb4c"], [-0.5, -5.5, 1, 1, "#ed88a5"], [-0.5, -4.5, 1, 0.5, "#d6deec"], [-0.5, -1, 1, 0.5, "#f0d1a2"], [0, -0.5, 0.5, 0.5, dark]]),
  magnifier: part("Magnifying glass", [[-0.5, -2, 1, 2, "#bd915f"], [-1, -5, 2, 0.5, dark], [-2, -4.5, 1, 2, dark], [1, -4.5, 1, 2, dark], [-1, -2.5, 2, 0.5, dark], [-1, -4.5, 2, 2, "#a9e4fa"], [-0.5, -4.5, 1, 0.5, "#effcff"]]),
  wrench: part("Spanner", [[-0.5, -3.5, 1, 3.5, "#6daec1"], [-1.5, -5, 1, 2, "#a9dce7"], [0.5, -5, 1, 2, "#a9dce7"], [-1.5, -3.5, 3, 1, "#6daec1"]]),
  mug: part("Coffee mug", [[-1.5, -3.5, 3, 3, paper], [-1, -3.5, 2, 0.5, "#765440"], [1.5, -3, 1, 0.5, paper], [2, -2.5, 0.5, 1, paper], [1.5, -1.5, 1, 0.5, paper], [-1, -0.5, 2, 0.5, "#d8c6a5"]]),
  paintbrush: part("Paintbrush", [[-0.5, -3, 1, 3, "#316f84"], [-0.5, -4, 1, 1, paper], [-0.5, -5.5, 1, 1.5, "#ee947e"], [0, -6, 0.5, 1, "#ee947e"]]),
  notebook: part("Burgundy notebook", [[-2, -5, 4, 4.5, "#8e3c5d"], [-1.5, -4.5, 3, 0.5, paper], [-1.5, -4, 0.5, 3.5, "#d88b9c"], [-0.5, -3.5, 1.5, 0.5, paper]]),
  map: part("Folded map", [[-2, -4, 1.5, 3.5, "#c9eee4"], [-0.5, -3.5, 1.5, 3.5, "#9bccc3"], [1, -4, 1.5, 3.5, "#c9eee4"], [-1.5, -2, 0.5, 0.5, dark], [-0.5, -1.5, 0.5, 0.5, dark], [0.5, -2, 0.5, 0.5, dark], [1.5, -2.5, 0.5, 0.5, dark]]),
  camera: part("Camera", [[-2, -3, 4, 2.5, "#a1a6b4"], [-1, -3.5, 1.5, 0.5, "#c7ccd7"], [-1.5, -2.5, 3, 1.5, dark], [-0.5, -2.5, 1, 1.5, "#75d1f4"], [0.5, -2, 0.5, 0.5, "#c4f5fc"]]),
  key: part("Brass key", [[-1, -5, 2, 0.5, gold], [-1.5, -4.5, 0.5, 1.5, gold], [1, -4.5, 0.5, 1.5, gold], [-1, -3, 2, 0.5, gold], [-0.5, -2.5, 1, 2.5, gold], [0.5, -1, 1, 0.5, gold]]),
  envelope: part("Envelope", [[-2, -3, 4, 2.5, paper], [-2, -3, 0.5, 0.5, "#ae946f"], [-1.5, -2.5, 0.5, 0.5, "#ae946f"], [-1, -2, 2, 0.5, "#ae946f"], [1, -2.5, 0.5, 0.5, "#ae946f"], [1.5, -3, 0.5, 0.5, "#ae946f"]]),
  hourglass: part("Hourglass", [[-1.5, -5, 3, 0.5, paper], [-1.5, -0.5, 3, 0.5, paper], [-1, -4.5, 0.5, 4, paper], [0.5, -4.5, 0.5, 4, paper], [-0.5, -4, 1, 0.5, gold], [0, -3.5, 0.5, 2, gold], [-0.5, -1.5, 1, 0.5, gold]]),
};

export const handStyles = {
  floating: part("Floating ball hands", [[-1, -1.5, 2, 0.5, "light"], [-1.5, -1, 3, 2, "base"], [-1, 1, 2, 0.5, "shade"], [-1, -1, 1, 0.5, "light"]]),
  none: part("No hands", []),
};

export const presetGroups = [
  { id: "pastel", label: "Pastel mix", reference: "approved/pastel-mix.png" },
  { id: "soft", label: "Soft mix", reference: "approved/soft-mix.png" },
  { id: "contrast", label: "Contrast mix", reference: "approved/contrast-mix.png" },
];

const preset = (id, group, cell, palette, expression, accessory, prop, heldSide) => ({ id, group, cell, body: "original", palette, expression, accessory, prop, hands: "floating", heldSide });
export const presets = [
  preset("mint-book", "pastel", "top-left", "mint", "square", "headset", "blue-book", "left"),
  preset("lilac-pencil", "pastel", "top-right", "lilac", "sleepy", "square-glasses", "pencil", "right"),
  preset("sky-magnifier", "pastel", "bottom-left", "sky", "cheerful", "sprout", "magnifier", "left"),
  preset("peach-wrench", "pastel", "bottom-right", "peach", "dot", "star-pin", "wrench", "right"),
  preset("butter-mug", "soft", "top-left", "butter", "square", "navy-cap", "mug", "left"),
  preset("cream-brush", "soft", "top-right", "cream", "sleepy", "leaf-pin", "paintbrush", "right"),
  preset("rose-notebook", "soft", "bottom-left", "rose", "cheerful", "bow", "notebook", "left"),
  preset("periwinkle-map", "soft", "bottom-right", "periwinkle", "dot", "round-glasses", "map", "right"),
  preset("ice-camera", "contrast", "top-left", "ice", "wink", "headset", "camera", "left"),
  preset("sage-key", "contrast", "top-right", "sage", "square", "badge", "key", "right"),
  preset("coral-envelope", "contrast", "bottom-left", "coral", "sleepy", "barrette", "envelope", "left"),
  preset("midnight-hourglass", "contrast", "bottom-right", "midnight", "square", "moon-pin", "hourglass", "right"),
];

export const catalog = { version: 1, rendering: "pixel-grid-preview", bodies, palettes, expressions, accessories, props, handStyles, presetGroups, presets };

export function hash32(value) {
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(String(value))) {
    hash = Math.imul(hash ^ byte, 0x01000193) >>> 0;
  }
  return hash;
}

// Each category has its own hash stream. Adding a choice affects only that category.
function pick(seed, category, choices) {
  return Object.keys(choices).reduce((best, id) =>
    best === null || hash32(`${seed}:${category}:${id}`) > hash32(`${seed}:${category}:${best}`) ? id : best, null);
}

export function traitsFromSeed(seed, overrides = {}) {
  return {
    body: pick(seed, "body", bodies),
    palette: pick(seed, "palette", palettes),
    expression: pick(seed, "expression", expressions),
    accessory: pick(seed, "accessory", accessories),
    prop: pick(seed, "prop", props),
    hands: "floating",
    heldSide: hash32(`${seed}:held-side`) % 2 ? "right" : "left",
    ...overrides,
  };
}

function rectangles(rects, palette) {
  return rects.map(([x, y, width, height, fill]) => `<rect x="${x}" y="${y}" width="${width}" height="${height}" fill="${palette[fill] ?? fill}"/>`).join("");
}

function place(layer, point, content) {
  return `<g data-layer="${layer}" transform="translate(${point.join(" ")})">${content}</g>`;
}

function bodyPath(body) {
  return body.rows.map(([x, width], row) => `M${x} ${body.firstRow + row}h${width}v1h-${width}z`).join("");
}

function bodyMarkup(body, palette, id) {
  return `<g data-layer="body"><path d="${bodyPath(body)}" fill="${palette.base}"/><g clip-path="url(#${id}-mask)">${rectangles(body.shade.map(r => [...r, "shade"]), palette)}${rectangles(body.highlight.map(r => [...r, "light"]), palette)}</g></g>`;
}

export function renderBodySvg(bodyId = "original", paletteId = "mint") {
  const body = bodies[bodyId];
  const palette = palettes[paletteId];
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${body.viewBox.join(" ")}" shape-rendering="crispEdges"><defs><clipPath id="body-mask"><path d="${bodyPath(body)}"/></clipPath></defs>${bodyMarkup(body, palette, "body")}</svg>`;
}

export function renderSvg(traits, { size = 240, id = "cloudlet", state = "idle" } = {}) {
  const body = bodies[traits.body];
  const palette = palettes[traits.palette];
  const expression = expressions[traits.expression];
  const accessory = accessories[traits.accessory];
  const prop = props[traits.prop];
  const hands = handStyles[traits.hands];
  if (!body || !palette || !expression || !accessory || !prop || !hands || !["left", "right"].includes(traits.heldSide)) {
    throw new Error("Unknown cloudlet trait");
  }
  if (!/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(id)) throw new Error("Invalid SVG id");
  if (!["idle", "hopping", "working", "waiting", "done", "attention"].includes(state)) throw new Error("Unknown state");
  const mask = bodyPath(body);
  const geometry = bodyMarkup(body, palette, id);
  const eyes = place("expression", body.anchors.face, rectangles(expression.rects, palette));
  const adornment = place("accessory", body.anchors[accessory.anchor ?? "accessory"], rectangles(accessory.rects, palette));
  const particles = `<g data-layer="particles" data-motion="particles">${body.particles.map((r, index) => `<g data-particle="${index}">${rectangles([[...r, "light"]], palette)}</g>`).join("")}</g>`;
  const hand = side => {
    if (traits.hands === "none") return "";
    const held = traits.heldSide === side ? place("prop", [0, -1], rectangles(prop.rects, palette)) : "";
    return `<g data-motion="${side}-hand">${place(`${side}-hand`, body.anchors[`${side}Hand`], `${rectangles(hands.rects, palette)}${held}`)}</g>`;
  };
  const actor = `<g data-motion="actor"><g data-motion="cloud">${geometry}${eyes}${adornment}</g>${hand("left")}${hand("right")}</g>`;
  const heldSide = traits.hands !== "none" && traits.prop !== "none" ? traits.heldSide : "none";
  const title = `${palette.label} cloudlet, ${expression.label.toLowerCase()}, ${accessory.label.toLowerCase()}, ${prop.label.toLowerCase()}`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${body.viewBox.join(" ")}" width="${size}" height="${size * body.viewBox[3] / body.viewBox[2]}" shape-rendering="crispEdges" role="img" aria-labelledby="${id}-title" data-state="${state}" data-held-side="${heldSide}"><title id="${id}-title">${title}</title><defs><clipPath id="${id}-mask"><path d="${mask}"/></clipPath></defs>${particles}${actor}</svg>`;
}

export function renderPartSvg(kind, id) {
  const collections = { expression: expressions, accessory: accessories, prop: props, hands: handStyles };
  const selected = collections[kind]?.[id];
  if (!selected) throw new Error("Unknown part");
  const viewBox = kind === "prop" ? "-4 -9 8 10" : kind === "expression" ? "-2 -2 12 6" : kind === "hands" ? "-2 -2 4 4" : "-4 -4 16 12";
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}" shape-rendering="crispEdges">${rectangles(selected.rects, palettes.mint)}</svg>`;
}
