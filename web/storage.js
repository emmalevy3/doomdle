// Each player's attempts per puzzle, kept in this browser only. Storage can
// be unavailable (private windows, blocked site data); the game still works,
// it just forgets attempts on reload.

const PREFIX = "doomdle:attempts:";

/** The saved record for a puzzle: { attempts: [{ pile, result }] }. */
export function loadRecord(puzzleId) {
  try {
    const raw = localStorage.getItem(PREFIX + puzzleId);
    const record = raw ? JSON.parse(raw) : null;
    if (record && Array.isArray(record.attempts)) {
      return record;
    }
  } catch {
    // Fall through to a fresh record.
  }
  return { attempts: [] };
}

export function saveRecord(puzzleId, record) {
  try {
    localStorage.setItem(PREFIX + puzzleId, JSON.stringify(record));
  } catch {
    // Not saved; the attempt still counts for this visit.
  }
}

const SETTING_PREFIX = "doomdle:setting:";

/** A saved display preference (a string), or `fallback` if none is saved. */
export function loadSetting(name, fallback) {
  try {
    return localStorage.getItem(SETTING_PREFIX + name) ?? fallback;
  } catch {
    return fallback;
  }
}

export function saveSetting(name, value) {
  try {
    localStorage.setItem(SETTING_PREFIX + name, value);
  } catch {
    // Not saved; the choice still applies for this visit.
  }
}
