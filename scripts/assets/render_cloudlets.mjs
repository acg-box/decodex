import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { catalog, presets, palettes, accessories, props, expressions, handStyles, renderSvg, renderBodySvg, renderPartSvg } from "../../assets/mascots/cloudlets/source/cloudlets.mjs";

const root = fileURLToPath(new URL("../../assets/mascots/cloudlets/", import.meta.url));
const generated = join(root, "generated");
await mkdir(join(generated, "combinations"), { recursive: true });
await mkdir(join(generated, "parts"), { recursive: true });

for (const preset of presets) {
  await writeFile(join(generated, "combinations", `${preset.id}.svg`), renderSvg(preset, {id:preset.id,size:320}) + "\n");
}
await writeFile(join(generated, "parts", "body-original.svg"), renderBodySvg() + "\n");
for (const [kind, collection] of Object.entries({ expression: expressions, accessory: accessories, prop: props, hands: handStyles })) {
  for (const [id, selected] of Object.entries(collection)) {
    if (selected.rects.length) await writeFile(join(generated, "parts", `${kind}-${id}.svg`), renderPartSvg(kind, id) + "\n");
  }
}
await writeFile(join(root, "catalog.json"), JSON.stringify(catalog, null, 2) + "\n");
const review = presets.map((preset, index) => {
  const x = 15 + index % 4 * 195;
  const y = 65 + Math.floor(index / 4) * 175;
  return `<g transform="translate(${x} ${y})">${renderSvg(preset,{id:`review-${preset.id}`,size:180})}<text x="90" y="145" fill="#bcb5cd" text-anchor="middle" font-family="sans-serif" font-size="12">${palettes[preset.palette].label}</text></g>`;
}).join("");
await writeFile(join(generated, "combinations.svg"), `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600"><rect width="800" height="600" fill="#0b0a0f"/><text x="30" y="40" fill="#f1eff7" font-family="sans-serif" font-size="25">CLOUDLETS — MIX &amp; MATCH</text>${review}</svg>\n`);
const cards = presets.map(preset => `<button class="preset" data-preset="${preset.id}" aria-pressed="false" aria-label="Load ${palettes[preset.palette].label}, ${props[preset.prop].label}">${renderSvg(preset,{id:`preset-${preset.id}`,size:240})}<strong>${palettes[preset.palette].label}</strong><span>${expressions[preset.expression].label} · ${accessories[preset.accessory].label} · ${props[preset.prop].label}</span></button>`).join("\n");
const source = (await readFile(join(root, "source", "cloudlets.mjs"), "utf8")).replace(/^export /gm, "");
const motion = await readFile(join(root, "source", "motion.css"), "utf8");
const template = await readFile(join(root, "source", "preview.html"), "utf8");
await writeFile(join(root, "preview.html"), template.replace("__PRESET_CARDS__", cards).replace("__CLOUDLET_SOURCE__", source).replace("__MOTION_CSS__", motion));
console.log(`Generated ${presets.length} combinations, separate SVG parts, catalog.json and a standalone preview.`);
