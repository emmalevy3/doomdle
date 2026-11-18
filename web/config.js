// Site-wide game settings. A puzzle file's "attempts" field overrides
// ATTEMPTS for that puzzle.

/** Piles a player may try per puzzle. */
export const ATTEMPTS = 3;

/** The first daily puzzle; the archive lists every day from here to today. */
export const FIRST_PUZZLE = "2026-11-16";

/** After the last failed attempt, show the authored solution and its line. */
export const REVEAL_ON_LOSS = true;
