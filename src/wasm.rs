//! The WebAssembly exports, built with `wasm-pack build --target web
//! --features wasm`. Thin wrappers over [`crate::api`], which does the work
//! and is tested natively.

use wasm_bindgen::prelude::*;

/// Verifies a pile (a JSON array of card names, top first) against a puzzle
/// file. See [`crate::api::verify_json`] for the result shape.
#[wasm_bindgen]
pub fn verify_pile(puzzle_json: &str, pile_json: &str) -> String {
    crate::api::verify_json(puzzle_json, pile_json)
}

/// Searches a puzzle file for a winning pile. See
/// [`crate::api::solve_json`] for the result shape.
#[wasm_bindgen]
pub fn solve_puzzle(puzzle_json: &str) -> String {
    crate::api::solve_json(puzzle_json)
}

/// Every card in the pool by printed name, as a JSON array (for tools such
/// as the card image fetcher).
#[wasm_bindgen]
pub fn card_names() -> String {
    crate::api::card_names_json()
}
