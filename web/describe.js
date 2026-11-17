// Turns the verifier's results into the sentences players read. Pure
// functions, tested in Node.

/** "A", "A and B", "A, B and C". */
export function listNames(names) {
  if (names.length <= 1) {
    return names.join("");
  }
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

function cards(n) {
  return n === 1 ? "1 card" : `${n} cards`;
}

/** How the closest line ended, as a sentence. */
export function closestSentence(closest) {
  const left = cards(closest.cards_left);
  switch (closest.ended) {
    case "you drew from an empty library":
      return "Closest line: you drew from an empty library.";
    case "the turn ends without a win":
      return `Closest line: you ran out of plays with ${left} left in your library.`;
    case "your life total hit 0":
      return `Closest line: your life total hit 0 with ${left} left in your library.`;
    case "the line depended on the order of a shuffled library":
      return `Closest line: it needed the order of a shuffled library, with ${left} left.`;
    default:
      return `Closest line: ${closest.ended}, with ${left} left in your library.`;
  }
}

/**
 * The one-sentence verdict on a verify result:
 * { ok, wins, gave_up, line?, feedback? } or { ok: false, error }.
 */
export function verdictSentence(result) {
  if (!result.ok) {
    return `Something went wrong checking this pile: ${result.error}`;
  }
  if (result.wins) {
    return "This pile wins.";
  }
  if (result.gave_up) {
    return "The check ran out of time on this pile, so it doesn't count as an attempt.";
  }
  const feedback = result.feedback ?? {};
  const stoppedBy = feedback.stopped_by ?? [];
  if (stoppedBy.length === 1) {
    return `This pile wins unless they have ${stoppedBy[0]}.`;
  }
  if (stoppedBy.length > 1) {
    return `This pile wins unless they have ${listNames(stoppedBy)} together.`;
  }
  if (feedback.wins_without_their_hand) {
    return "This pile would win against an empty hand, but they have more than one answer to it.";
  }
  if (feedback.closest) {
    return closestSentence(feedback.closest);
  }
  return "This pile doesn't win.";
}

/** Life after Doomsday's loss of half, rounded up. */
export function lifeAfterDoomsday(life) {
  return life - Math.ceil(Math.max(life, 0) / 2);
}

/** Today's date in the player's own time zone, as YYYY-MM-DD. */
export function localDate(now = new Date()) {
  const pad = (n) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

// ---------------------------------------------------------------- sharing

const SQUARE = { win: "🟩", answered: "🟨", reached: "🟥", unreached: "⬛" };

/** The attempts that count: checked, not given up. */
function counted(attempts) {
  return attempts.filter((a) => a.result.ok && !a.result.gave_up);
}

/**
 * One row of squares per counted attempt, one square per pile card:
 * all green for a win, all yellow for a pile only the opponent's hand stops,
 * otherwise red for each pile card the closest line got through, then black.
 */
export function shareRows(attempts, pileSize) {
  return counted(attempts).map(({ result }) => {
    if (result.wins) {
      return SQUARE.win.repeat(pileSize);
    }
    const feedback = result.feedback ?? {};
    if ((feedback.stopped_by ?? []).length > 0 || feedback.wins_without_their_hand) {
      return SQUARE.answered.repeat(pileSize);
    }
    const left = feedback.closest?.cards_left;
    const reached = left === undefined ? 0 : Math.min(pileSize, Math.max(0, pileSize - left));
    return SQUARE.reached.repeat(reached) + SQUARE.unreached.repeat(pileSize - reached);
  });
}

/**
 * "Doomdle 2026-11-16 (Hard) 2/3", or "X/3" when every attempt missed. The
 * tier is left out when the puzzle has none.
 */
function shareHeader(id, attempts, allowed, difficulty) {
  const tries = counted(attempts);
  const winAt = tries.findIndex((a) => a.result.wins);
  const tier = difficulty ? ` (${difficulty.charAt(0).toUpperCase()}${difficulty.slice(1)})` : "";
  return `Doomdle ${id}${tier} ${winAt === -1 ? "X" : winAt + 1}/${allowed}`;
}

/** The plain result to paste anywhere. */
export function shareText({ id, attempts, allowed, difficulty, pileSize, link }) {
  return [shareHeader(id, attempts, allowed, difficulty), ...shareRows(attempts, pileSize), link].join(
    "\n",
  );
}

/**
 * The result for Discord: after a win, the winning pile follows in a spoiler.
 * (The full line is too long to share; the pile is the answer.)
 */
export function discordText(details) {
  const plain = shareText(details);
  const { attempts } = details;
  const win = counted(attempts).find((a) => a.result.wins);
  if (!win) {
    return plain;
  }
  return `${plain}\nMy winning pile: ||${win.pile.join(" → ")}||`;
}
