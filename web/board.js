// Draws cards and the puzzle's game state.

import { lifeAfterDoomsday } from "./describe.js";

let images = {};
let zoom = null;

/** The name to Scryfall image map, loaded once by app.js. */
export function setImages(map) {
  images = map ?? {};
}

/** The dialog that shows a card large. */
export function setZoomDialog(dialog) {
  zoom = dialog;
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  if (text !== undefined) {
    node.textContent = text;
  }
  return node;
}

/**
 * One card: its Scryfall image, or a text tile if there is none. `badges` are
 * short labels shown on the card (tapped, loyalty, ...).
 */
export function cardElement(name, { badges = [], onClick } = {}) {
  const card = el("button", "card");
  card.type = "button";
  card.title = name;
  const image = images[name];
  if (image?.normal) {
    const img = el("img");
    img.src = image.normal;
    img.alt = name;
    img.loading = "lazy";
    card.append(img);
  } else {
    card.classList.add("text-card");
    card.append(el("span", "card-name", name));
  }
  if (badges.length > 0) {
    const row = el("span", "badges");
    for (const badge of badges) {
      row.append(el("span", "badge", badge));
    }
    card.append(row);
  }
  card.addEventListener("click", onClick ?? (() => showLarge(name)));
  return card;
}

function showLarge(name) {
  if (!zoom) {
    return;
  }
  const body = zoom.querySelector(".zoom-body");
  body.replaceChildren();
  const image = images[name];
  if (image?.large || image?.normal) {
    const img = el("img");
    img.src = image.large ?? image.normal;
    img.alt = name;
    body.append(img);
  } else {
    body.append(el("p", "zoom-name", name));
  }
  zoom.showModal();
}

/** A battlefield entry from the puzzle file: a name or { card, ... }. */
function permanent(entry) {
  if (typeof entry === "string") {
    return { card: entry };
  }
  return entry;
}

function permanentBadges(p) {
  const badges = [];
  if (p.tapped) badges.push("tapped");
  if (p.summoning_sick) badges.push("summoning sick");
  if (p.loyalty !== undefined) badges.push(`loyalty ${p.loyalty}`);
  if (p.type) badges.push(p.type);
  if (p.greaves) badges.push("Greaves");
  if (p.skateboard) badges.push("Skateboard");
  return badges;
}

function zone(label, entries, toCard) {
  const section = el("section", "zone");
  section.append(el("h3", null, label));
  const row = el("div", "cards");
  if (entries.length === 0) {
    row.append(el("p", "empty", "Nothing"));
  }
  for (const entry of entries) {
    row.append(toCard(entry));
  }
  section.append(row);
  return section;
}

function facts(items) {
  const list = el("ul", "facts");
  for (const item of items) {
    list.append(el("li", null, item));
  }
  return list;
}

/** Your side and the opponent's, from the parsed puzzle file. */
export function renderBoard(container, puzzle) {
  const you = puzzle.you ?? {};
  const them = puzzle.opponent ?? {};
  const life = you.life ?? 20;
  container.replaceChildren();

  const mine = el("div", "side");
  mine.append(el("h2", null, "You"));
  const yourFacts = [
    `Life ${life}, ${lifeAfterDoomsday(life)} after Doomsday`,
    you.land_drop === false ? "Land drop used" : "Land drop available",
  ];
  if (you.mana_pool) yourFacts.push(`Floating mana: ${you.mana_pool}`);
  if (you.spells_cast) yourFacts.push(`Spells cast before Doomsday: ${you.spells_cast}`);
  if (you.cards_drawn) yourFacts.push(`Cards drawn this turn: ${you.cards_drawn}`);
  if (you.yawgmoths_will) yourFacts.push("Yawgmoth's Will is active");
  mine.append(facts(yourFacts));
  mine.append(zone("Hand", you.hand ?? [], (name) => cardElement(name)));
  mine.append(
    zone("Battlefield", (you.battlefield ?? []).map(permanent), (p) =>
      cardElement(p.card, { badges: permanentBadges(p) }),
    ),
  );

  const theirs = el("div", "side opponent");
  theirs.append(el("h2", null, "Opponent"));
  theirs.append(facts([`Life ${them.life ?? 20}`]));
  theirs.append(zone("Hand (open)", them.hand ?? [], (name) => cardElement(name)));
  theirs.append(
    zone("Battlefield", (them.battlefield ?? []).map(permanent), (p) =>
      cardElement(p.card, { badges: permanentBadges(p) }),
    ),
  );

  container.append(mine, theirs);
}
