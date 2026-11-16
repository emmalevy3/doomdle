// Node checks of the router's decisions (no Cloudflare needed).
import assert from "node:assert/strict";
import { route, rewriteLocation } from "./worker.js";

const env = {
  DOOMDLE_ORIGIN: "https://doomdle.pages.dev",
  EMMADLE_ORIGIN: "https://emmadle.example.workers.dev/",
};
const at = (url) => route(new URL(url), env);

assert.deepEqual(at("https://emmanems.com/"), { kind: "home" });
assert.deepEqual(at("https://emmanems.com/index.html"), { kind: "home" });
assert.deepEqual(at("https://emmanems.com/doomdle"), {
  kind: "redirect",
  location: "https://emmanems.com/doomdle/",
});
assert.deepEqual(at("https://emmanems.com/emmadle?x=1"), {
  kind: "redirect",
  location: "https://emmanems.com/emmadle/?x=1",
});
assert.deepEqual(at("https://emmanems.com/doomdle/"), {
  kind: "proxy",
  target: "https://doomdle.pages.dev/",
  prefix: "/doomdle",
  origin: "https://doomdle.pages.dev",
});
assert.equal(
  at("https://emmanems.com/doomdle/pkg/doomdle_bg.wasm").target,
  "https://doomdle.pages.dev/pkg/doomdle_bg.wasm",
);
assert.equal(
  at("https://emmanems.com/doomdle/?puzzle=2026-11-20&view=phone").target,
  "https://doomdle.pages.dev/?puzzle=2026-11-20&view=phone",
);
// A trailing slash on the configured origin is not doubled.
assert.equal(
  at("https://emmanems.com/emmadle/gameData.json").target,
  "https://emmadle.example.workers.dev/gameData.json",
);
assert.deepEqual(at("https://www.emmanems.com/doomdle/?a=b"), {
  kind: "redirect",
  location: "https://emmanems.com/doomdle/?a=b",
});
assert.deepEqual(at("https://emmanems.com/doomdlex"), { kind: "notfound" });
assert.deepEqual(at("https://emmanems.com/other"), { kind: "notfound" });
// A missing origin is a 404, not a request to nowhere.
assert.deepEqual(route(new URL("https://emmanems.com/doomdle/"), {}), { kind: "notfound" });

assert.equal(rewriteLocation("/", "/doomdle", "https://doomdle.pages.dev"), "/doomdle/");
assert.equal(
  rewriteLocation("https://doomdle.pages.dev/app", "/doomdle", "https://doomdle.pages.dev"),
  "/doomdle/app",
);
assert.equal(
  rewriteLocation("https://elsewhere.org/", "/doomdle", "https://doomdle.pages.dev"),
  "https://elsewhere.org/",
);
assert.equal(rewriteLocation("//cdn.org/x", "/doomdle", "https://doomdle.pages.dev"), "//cdn.org/x");

console.log("router: all checks passed");
