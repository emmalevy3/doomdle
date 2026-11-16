// Draws cards and the puzzle's game state.

import { lifeAfterDoomsday } from "./describe.js";

let images = {};
let zoom = null;

/** The name to Scryfall image map, loaded once by app.js. */
export function setImages(map) {
  images = map ?? {};
}

/** The largest image URL for `name`, or undefined if there is none. */
export function largeImage(name) {
  return images[name]?.large ?? images[name]?.normal;
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
 * short labels shown on the card (loyalty, Cavern type, ...).
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
  if (p.summoning_sick) badges.push("summoning sick");
  if (p.loyalty !== undefined) badges.push(`loyalty ${p.loyalty}`);
  if (p.type) badges.push(p.type);
  if (p.greaves) badges.push("Greaves");
  if (p.skateboard) badges.push("Skateboard");
  return badges;
}

function zone(label, entries, toCard) {
  const section = el("section", "zone");
  section.append(el("h3", null, label), cardRow(entries, toCard));
  return section;
}

/** A battlefield card, turned sideways in a square frame when tapped. */
function permanentCard(p) {
  const card = cardElement(p.card, { badges: permanentBadges(p) });
  if (!p.tapped) {
    return card;
  }
  card.classList.add("tapped");
  card.title = `${p.card} (tapped)`;
  const frame = el("span", "tapped-frame");
  frame.append(card);
  return frame;
}

/** Small labelled values: [label, value] pairs. */
function stats(items) {
  const list = el("ul", "stats");
  for (const [label, value] of items) {
    const item = el("li", "stat");
    item.append(el("span", "stat-label", label), el("span", "stat-value", value));
    list.append(item);
  }
  return list;
}

/** A panel: a heading, then whatever `children` are. */
function panel(target, heading, ...children) {
  target.replaceChildren(el("h2", null, heading), ...children);
}

function cardRow(entries, toCard) {
  const row = el("div", "cards");
  if (entries.length === 0) {
    row.append(el("p", "empty", "Nothing"));
  }
  for (const entry of entries) {
    row.append(toCard(entry));
  }
  return row;
}

/**
 * The game state from the parsed puzzle file, drawn into three panels:
 * `you` (your stats and battlefield), `hand` (your hand) and `them` (the
 * opponent's stats, revealed hand and battlefield).
 */
export function renderBoard({ you: yourPanel, hand: handPanel, them: theirPanel }, puzzle) {
  const you = puzzle.you ?? {};
  const them = puzzle.opponent ?? {};
  const life = you.life ?? 20;

  const yourStats = [
    ["Life", `${life} → ${lifeAfterDoomsday(life)}`],
    ["Land drop", you.land_drop === false ? "used" : "✓"],
  ];
  if (you.mana_pool) yourStats.push(["Floating", you.mana_pool]);
  if (you.spells_cast) yourStats.push(["Spells cast", String(you.spells_cast)]);
  if (you.cards_drawn) yourStats.push(["Cards drawn", String(you.cards_drawn)]);
  if (you.yawgmoths_will) yourStats.push(["Yawgmoth's Will", "active"]);
  panel(
    yourPanel,
    "Your board",
    stats(yourStats),
    zone("Battlefield", (you.battlefield ?? []).map(permanent), permanentCard),
  );

  panel(handPanel, "Your hand", cardRow(you.hand ?? [], (name) => cardElement(name)));

  panel(
    theirPanel,
    "Opponent",
    stats([["Life", String(them.life ?? 20)]]),
    zone("Hand (revealed)", them.hand ?? [], (name) => cardElement(name)),
    zone("Battlefield", (them.battlefield ?? []).map(permanent), permanentCard),
  );
}
