// The request handler the worker runs. Kept apart from worker.js so it can
// be tested in Node against the same wasm package.

/**
 * Answers one request with the wasm API (`verify_pile`, `solve_puzzle`).
 * request: { id, kind: "verify" | "solve", puzzle: string, pile?: string[] }
 * Returns { id, result } where result is the API's parsed JSON. Never throws.
 */
export function handleRequest(api, request) {
  const { id, kind, puzzle, pile } = request;
  let text;
  try {
    if (kind === "verify") {
      text = api.verify_pile(puzzle, JSON.stringify(pile));
    } else if (kind === "solve") {
      text = api.solve_puzzle(puzzle);
    } else {
      text = JSON.stringify({ ok: false, error: `unknown request: ${kind}` });
    }
  } catch (error) {
    text = JSON.stringify({ ok: false, error: String(error) });
  }
  return { id, result: JSON.parse(text) };
}
