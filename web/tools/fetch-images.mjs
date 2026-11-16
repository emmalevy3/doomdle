// Fetches a Scryfall image URL for every card in the pool and writes
// web/card-images.json. Run by hand when the card pool changes (it needs the
// network and the wasm build for the card names):
//
//     wasm-pack build --target web --out-dir web/pkg --features wasm
//     node web/tools/fetch-images.mjs
//
// The site then hotlinks Scryfall's image CDN from that file; visitors never
// call the Scryfall API. Requests are spaced 100 ms apart, as Scryfall asks.

import { readFileSync, writeFileSync } from "node:fs";
import init, { card_names } from "../pkg/doomdle.js";

await init({
  module_or_path: readFileSync(new URL("../pkg/doomdle_bg.wasm", import.meta.url)),
});
const names = JSON.parse(card_names());

const headers = { "User-Agent": "Doomdle/0.1", Accept: "application/json" };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// One request, retried with a growing wait when Scryfall says to slow down
// (HTTP 429). Returns the response, or null after too many tries.
async function fetchCard(url) {
  for (let attempt = 0; attempt < 6; attempt++) {
    const response = await fetch(url, { headers });
    if (response.status !== 429) {
      return response;
    }
    const wait = Number(response.headers.get("retry-after")) * 1000 || 1000 * 2 ** attempt;
    await sleep(wait);
  }
  return null;
}

const images = {};
const missing = [];
const failed = [];
for (const name of names) {
  const url = `https://api.scryfall.com/cards/named?exact=${encodeURIComponent(name)}`;
  const response = await fetchCard(url);
  await sleep(150);
  if (response === null || (!response.ok && response.status !== 404)) {
    failed.push(`${name} (${response?.status ?? "rate limited"})`);
    continue;
  }
  if (!response.ok) {
    missing.push(name);
    continue;
  }
  const card = await response.json();
  // A double-faced card (Tamiyo) keeps its images per face: use the face
  // with this name.
  const face = (card.card_faces ?? []).find((f) => f.name === name && f.image_uris);
  const uris = face?.image_uris ?? card.image_uris;
  if (!uris) {
    missing.push(name);
    continue;
  }
  images[name] = { normal: uris.normal, large: uris.large };
}

const out = new URL("../card-images.json", import.meta.url);
writeFileSync(out, JSON.stringify(images, null, 2) + "\n");
console.log(`${Object.keys(images).length} of ${names.length} cards written to web/card-images.json`);
if (missing.length > 0) {
  console.log(`Not on Scryfall: ${missing.join(", ")} (the site shows a text tile)`);
}
if (failed.length > 0) {
  console.error(`Requests failed, run again: ${failed.join(", ")}`);
  process.exit(1);
}
