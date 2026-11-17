// The emmanems.com router: a Cloudflare Worker that serves a small landing
// page at the root and forwards each game's path to where that game is hosted.
//
//   emmanems.com/            -> the landing page below
//   emmanems.com/doomdle/... -> DOOMDLE_ORIGIN/...   (Cloudflare Pages)
//   emmanems.com/emmadle/... -> EMMADLE_ORIGIN/...   (Emmadle's own Worker)
//   www.emmanems.com/...     -> emmanems.com/...     (301)
//
// The games use only relative paths, so they work under their prefix as is.

/** The games by path prefix, with the Worker variable naming their host. */
const GAMES = [
  ["/doomdle", "DOOMDLE_ORIGIN"],
  ["/emmadle", "EMMADLE_ORIGIN"],
];

/**
 * What to do with a request for `url` (a URL), given the Worker's `env`:
 * { kind: "home" }, { kind: "redirect", location }, { kind: "proxy", target,
 * prefix, origin } or { kind: "notfound" }. Pure, so it can be tested in Node.
 */
export function route(url, env) {
  if (url.hostname.startsWith("www.")) {
    const bare = new URL(url);
    bare.hostname = url.hostname.slice(4);
    return { kind: "redirect", location: bare.toString() };
  }
  if (url.pathname === "/" || url.pathname === "/index.html") {
    return { kind: "home" };
  }
  for (const [prefix, variable] of GAMES) {
    const origin = (env[variable] ?? "").replace(/\/+$/, "");
    if (url.pathname === prefix) {
      return { kind: "redirect", location: `${url.origin}${prefix}/${url.search}` };
    }
    if (origin && url.pathname.startsWith(`${prefix}/`)) {
      const rest = url.pathname.slice(prefix.length);
      return { kind: "proxy", target: `${origin}${rest}${url.search}`, prefix, origin };
    }
  }
  return { kind: "notfound" };
}

/**
 * A redirect from the upstream points at its own root; move it under the
 * game's prefix so the visitor stays on emmanems.com.
 */
export function rewriteLocation(location, prefix, origin) {
  if (location.startsWith(origin)) {
    return prefix + (location.slice(origin.length) || "/");
  }
  if (location.startsWith("/") && !location.startsWith("//")) {
    return prefix + location;
  }
  return location;
}

const HOME = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="theme-color" content="#fff4f8">
<title>emmanems</title>
<link rel="icon" href="data:image/svg+xml,<svg xmlns=%22http://www.w3.org/2000/svg%22 viewBox=%220 0 100 100%22><text y=%22.9em%22 font-size=%2290%22>🌷</text></svg>">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Fredoka:wght@500;600;700&family=Nunito:wght@500;700&display=swap">
<style>
  :root {
    --bg: #fff4f8; --bg-2: #f3efff; --text: #4a3b5c; --muted: #8a7a9b;
    --card: #ffffff; --ring: #f6d6e6; --shadow: rgb(180 140 200 / 0.25);
    --pink: #ff9cc2; --pink-soft: #ffe3ef; --blue: #8ec9ff; --blue-soft: #e2f1ff;
    --yellow: #ffd88a;
    color-scheme: light;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --bg: #231d33; --bg-2: #1b1a2e; --text: #f6ecff; --muted: #b9a9cc;
      --card: #2f2745; --ring: #4a3e63; --shadow: rgb(0 0 0 / 0.35);
      --pink-soft: #4a2f48; --blue-soft: #2b3a57;
      color-scheme: dark;
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; min-height: 100vh; padding: 56px 16px 40px;
    background:
      radial-gradient(circle at 12% 18%, var(--pink-soft) 0 90px, transparent 91px),
      radial-gradient(circle at 88% 12%, var(--blue-soft) 0 70px, transparent 71px),
      radial-gradient(circle at 80% 88%, var(--pink-soft) 0 110px, transparent 111px),
      linear-gradient(160deg, var(--bg), var(--bg-2)) fixed;
    color: var(--text); font: 500 17px/1.5 Nunito, "Segoe UI", sans-serif;
  }
  main { max-width: 520px; margin: 0 auto; text-align: center; }
  .sparkles { margin: 0; font-size: 1.6rem; letter-spacing: 0.3em; }
  h1 {
    margin: 6px 0 4px; font: 700 clamp(2.4rem, 11vw, 3.4rem)/1 Fredoka, sans-serif;
    color: var(--pink); text-shadow: 3px 3px 0 var(--yellow);
  }
  .tagline { margin: 0 0 30px; color: var(--muted); font-weight: 700; }
  a.game {
    display: flex; align-items: center; gap: 16px; margin: 0 0 18px; padding: 18px 20px;
    border: 3px solid var(--ring); border-radius: 26px; background: var(--card);
    color: inherit; text-decoration: none; text-align: left;
    box-shadow: 0 8px 0 var(--ring), 0 14px 28px var(--shadow);
    transition: transform 160ms ease, box-shadow 160ms ease, border-color 160ms ease;
  }
  a.game:hover, a.game:focus-visible {
    transform: translateY(-4px) rotate(-1deg); outline: none;
    box-shadow: 0 12px 0 var(--ring), 0 18px 30px var(--shadow);
  }
  a.game.doomdle:hover, a.game.doomdle:focus-visible { border-color: var(--blue); }
  a.game.emmadle:hover, a.game.emmadle:focus-visible { border-color: var(--pink); }
  .icon {
    flex: none; display: grid; place-items: center; width: 64px; height: 64px;
    border-radius: 20px; font-size: 2rem;
  }
  .doomdle .icon { background: var(--blue-soft); }
  .emmadle .icon { background: var(--pink-soft); }
  .name { display: block; font: 600 1.45rem/1.2 Fredoka, sans-serif; }
  .doomdle .name { color: var(--blue); }
  .emmadle .name { color: var(--pink); }
  .about { display: block; color: var(--muted); font-size: 0.98rem; }
  footer { margin-top: 30px; color: var(--muted); font-size: 0.9rem; }
  @media (prefers-reduced-motion: reduce) {
    a.game { transition: none; }
    a.game:hover, a.game:focus-visible { transform: none; }
  }
</style>
</head>
<body>
<main>
  <p class="sparkles" aria-hidden="true">✿ ♡ ✿</p>
  <h1>emmanems</h1>
  <p class="tagline">little daily puzzles ♡</p>
  <a class="game doomdle" href="/doomdle/">
    <span class="icon" aria-hidden="true">🃏</span>
    <span>
      <span class="name">Doomdle</span>
      <span class="about">A daily Magic: The Gathering puzzle: build the Doomsday pile that wins this turn.</span>
    </span>
  </a>
  <a class="game emmadle" href="/emmadle/">
    <span class="icon" aria-hidden="true">🎬</span>
    <span>
      <span class="name">Emmadle</span>
      <span class="about">A daily game from my Letterboxd reviews: which film did I rate higher?</span>
    </span>
  </a>
  <footer>new puzzles every day ✿</footer>
</main>
</body>
</html>
`;

export default {
  async fetch(request, env) {
    const decision = route(new URL(request.url), env);
    switch (decision.kind) {
      case "home":
        return new Response(HOME, {
          headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-cache" },
        });
      case "redirect":
        return Response.redirect(decision.location, 301);
      case "proxy": {
        const upstream = await fetch(new Request(decision.target, request), {
          redirect: "manual",
        });
        const location = upstream.headers.get("location");
        if (!location) {
          return upstream;
        }
        const response = new Response(upstream.body, upstream);
        response.headers.set(
          "location",
          rewriteLocation(location, decision.prefix, decision.origin),
        );
        return response;
      }
      default:
        return new Response("Not found", { status: 404 });
    }
  },
};
