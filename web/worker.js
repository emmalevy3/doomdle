// Runs the verifier off the main thread, so a long search never freezes the
// page. Messages in and out are described in verify.js.

import init, * as api from "./pkg/doomdle.js";
import { handleRequest } from "./verify.js";

const ready = init();

self.addEventListener("message", async (event) => {
  await ready;
  self.postMessage(handleRequest(api, event.data));
});
