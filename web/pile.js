// The pile builder: pick up to five candidates and order them, top first.

import { cardElement, hidePeek, showPeek } from "./board.js";

/**
 * The cards a pile can be built from: the library candidates, then the
 * graveyard, one entry per copy.
 */
export function candidates(puzzle) {
  const you = puzzle.you ?? {};
  return [
    ...(you.library ?? []).map((name) => ({ name, from: "library" })),
    ...(you.graveyard ?? []).map((name) => ({ name, from: "graveyard" })),
  ];
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/**
 * True on a touch screen (no hover) wide enough for the stacked layout, e.g.
 * a tablet held sideways: there the first tap on a candidate shows it and a
 * second tap picks it. Phones keep one tap to pick.
 */
function tapToPreview() {
  return window.matchMedia("(hover: none) and (min-width: 1000px)").matches;
}

function slotLabel(i) {
  return i === 0 ? "1 · top" : String(i + 1);
}

/** A read-only row of slots showing `names`, top first (used to reveal a pile). */
export function pileRow(names) {
  const slots = el("ol", "slots");
  names.forEach((name, i) => {
    const slot = el("li", "slot filled");
    slot.append(el("span", "slot-label", slotLabel(i)), cardElement(name));
    slots.append(slot);
  });
  return slots;
}

// ---------------------------------------------------------------- dragging

/** True for a short moment after a drag ends, so the drop isn't also a click. */
let suppressClick = false;
document.addEventListener(
  "click",
  (event) => {
    if (suppressClick) {
      event.stopPropagation();
      event.preventDefault();
      suppressClick = false;
    }
  },
  true,
);

/**
 * Lets the mouse drag `node` onto any element marked `data-drop`, then calls
 * `onDrop(thatElement.dataset.drop)`. Hand-rolled with pointer events rather
 * than HTML drag and drop, which Firefox doesn't allow on buttons. Mouse only:
 * on touch screens a drag would fight scrolling, so they keep tapping.
 */
function draggable(node, canDrag, onDrop) {
  node.addEventListener("pointerdown", (down) => {
    if (down.pointerType !== "mouse" || down.button !== 0 || !canDrag()) {
      return;
    }
    // Stops the image's own drag and text selection; the click still fires.
    down.preventDefault();
    const box = node.getBoundingClientRect();
    const grab = { x: down.clientX - box.left, y: down.clientY - box.top };
    let ghost = null;
    let target = null;

    const move = (event) => {
      if (!ghost) {
        if (Math.hypot(event.clientX - down.clientX, event.clientY - down.clientY) < 6) {
          return;
        }
        hidePeek();
        ghost = node.cloneNode(true);
        ghost.classList.add("drag-ghost");
        ghost.style.width = `${box.width}px`;
        document.body.append(ghost);
        document.body.classList.add("dragging");
      }
      ghost.style.left = `${event.clientX - grab.x}px`;
      ghost.style.top = `${event.clientY - grab.y}px`;
      const over = document.elementFromPoint(event.clientX, event.clientY)?.closest("[data-drop]");
      if (over !== target) {
        target?.classList.remove("drop-target");
        target = over ?? null;
        target?.classList.add("drop-target");
      }
    };
    const end = (drop) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      if (!ghost) {
        return;
      }
      ghost.remove();
      document.body.classList.remove("dragging");
      target?.classList.remove("drop-target");
      suppressClick = true;
      setTimeout(() => (suppressClick = false), 0);
      if (drop && target) {
        onDrop(target.dataset.drop);
      }
    };
    const up = () => end(true);
    const cancel = () => end(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
  });
}

/** True where a mouse can hover, i.e. where dragging is offered. */
function canHover() {
  return window.matchMedia("(hover: hover)").matches;
}

// ---------------------------------------------------------------- the builder

/**
 * A builder drawn into two panels: `pool` (the cards to pick from, one
 * overlapping stack) and `pile` (the slots, top first, and the buttons).
 * Slots may be filled in any order and left empty in between. A click fills
 * the first empty slot; with a mouse, cards can also be dragged into any
 * slot, between slots, or back to the stack. `onSubmit(pile)` gets the card
 * names, top first. Call `setEnabled(false)` once the game is over.
 */
export function createPileBuilder({ pool: poolPanel, pile: pilePanel }, puzzle, onSubmit) {
  const pool = candidates(puzzle);
  const size = Math.min(5, pool.length);
  let slots = Array(size).fill(null); // an index into pool, or null, top first
  let enabled = true;
  let previewed = null; // the candidate a first tap is showing (tapToPreview)

  // A tap anywhere outside the stack closes a tap preview.
  document.addEventListener("click", (event) => {
    if (previewed !== null && !poolPanel.contains(event.target)) {
      previewed = null;
      hidePeek();
    }
  });
  poolPanel.dataset.drop = "pool";

  const full = () => slots.every((s) => s !== null);
  const empty = () => slots.every((s) => s === null);

  /** Applies a drag of `from` ({ pool: index } or { slot: i }) onto `drop`. */
  function dropped(from, drop) {
    if (drop === "pool") {
      if (from.slot !== undefined) {
        slots[from.slot] = null;
      }
    } else {
      const to = Number(drop.replace("slot:", ""));
      if (from.slot !== undefined) {
        [slots[from.slot], slots[to]] = [slots[to], slots[from.slot]];
      } else {
        const at = slots.indexOf(from.pool);
        if (at !== -1) {
          slots[at] = null;
        }
        slots[to] = from.pool;
      }
    }
    render();
  }

  /**
   * The candidates as one stack. They stay real buttons that are never
   * `disabled`, so hovering still lifts a used card for reading; `add` refuses
   * what can't be picked.
   */
  function renderPool() {
    const stack = el("div", "pick-stack");
    pool.forEach((card, index) => {
      const used = slots.includes(index);
      const node = cardElement(card.name, { onClick: () => pick(index, node) });
      const pickable = enabled && !used && !full();
      node.setAttribute("aria-disabled", String(!pickable));
      node.title = used ? `${card.name} (in your pile)` : card.name;
      if (used) node.classList.add("used");
      node.addEventListener("mouseenter", () => showPeek(card.name, node));
      node.addEventListener("focus", () => showPeek(card.name, node));
      node.addEventListener("mouseleave", hidePeek);
      node.addEventListener("blur", hidePeek);
      draggable(
        node,
        () => enabled,
        (drop) => dropped({ pool: index }, drop),
      );
      stack.append(node);
    });
    const hint = tapToPreview()
      ? [el("p", "hint", "Tap to read, tap again to add.")]
      : canHover()
        ? [el("p", "hint", "Click to add, or drag to any slot.")]
        : [];
    poolPanel.replaceChildren(el("h2", null, "Cards to pick from"), ...hint, stack);
  }

  function render() {
    hidePeek();
    previewed = null;
    renderPool();

    const list = el("ol", "slots");
    for (let i = 0; i < size; i++) {
      const index = slots[i];
      const slot = el("li", index === null ? "slot" : "slot filled");
      slot.dataset.drop = `slot:${i}`;
      slot.append(el("span", "slot-label", slotLabel(i)));
      if (index === null) {
        slot.append(el("span", "slot-empty", "—"));
      } else {
        const card = cardElement(pool[index].name);
        draggable(
          card,
          () => enabled,
          (drop) => dropped({ slot: i }, drop),
        );
        slot.append(card);
        const controls = el("span", "slot-controls");
        const button = (label, title, action, disabled) => {
          const b = el("button", "small", label);
          b.type = "button";
          b.title = title;
          b.setAttribute("aria-label", title);
          b.disabled = disabled || !enabled;
          b.addEventListener("click", action);
          controls.append(b);
        };
        button("↑", "Move up", () => swap(i, i - 1), i === 0);
        button("↓", "Move down", () => swap(i, i + 1), i === size - 1);
        button("✕", "Remove", () => remove(i), false);
        slot.append(controls);
      }
      list.append(slot);
    }

    const submit = el("button", "primary", "Submit pile");
    submit.type = "button";
    submit.disabled = !enabled || !full();
    submit.addEventListener("click", () => onSubmit(slots.map((i) => pool[i].name)));

    const clear = el("button", "quiet", "Clear");
    clear.type = "button";
    clear.disabled = !enabled || empty();
    clear.addEventListener("click", () => {
      slots = Array(size).fill(null);
      render();
    });

    const actions = el("div", "actions");
    actions.append(submit, clear);

    pilePanel.replaceChildren(
      el("h2", null, "Doomsday pile"),
      el("p", "hint", `Choose ${size} card${size === 1 ? "" : "s"} and order them, top first.`),
      list,
      actions,
    );
  }

  /** A click on a candidate: on a tablet, show it first; otherwise pick it. */
  function pick(index, node) {
    if (tapToPreview() && previewed !== index) {
      previewed = index;
      showPeek(pool[index].name, node);
      return;
    }
    add(index);
  }

  /** Puts a candidate in the first empty slot. */
  function add(index) {
    const free = slots.indexOf(null);
    if (enabled && free !== -1 && !slots.includes(index)) {
      slots[free] = index;
      render();
    }
  }

  function remove(i) {
    slots[i] = null;
    render();
  }

  function swap(a, b) {
    [slots[a], slots[b]] = [slots[b], slots[a]];
    render();
  }

  render();
  return {
    setEnabled(value) {
      enabled = value;
      render();
    },
    clear() {
      slots = Array(size).fill(null);
      render();
    },
  };
}
