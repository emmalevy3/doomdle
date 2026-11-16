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
<meta name="theme-color" content="#0b0d12">
<title>emmanems</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Cinzel:wght@500;700&family=Cormorant+Garamond:ital,wght@0,500;1,500&display=swap">
<style>
  :root { --bg: #0b0d12; --surface: #141721; --text: #ece4d2; --muted: #8d8a85;
          --gold: #c6a15b; --gold-dim: #4f4430; --glow: #5b8fd6; color-scheme: dark; }
  * { box-sizing: border-box; }
  body { margin: 0; min-height: 100vh; padding: 48px 16px; background:
         radial-gradient(ellipse at 50% 0%, #151c2c 0%, var(--bg) 45%, #05060a 100%) fixed;
         color: var(--text); font: 500 18px/1.5 "Cormorant Garamond", Georgia, serif; }
  main { max-width: 560px; margin: 0 auto; }
  h1 { margin: 0 0 6px; text-align: center; font: 700 clamp(1.8rem, 7vw, 2.6rem)/1 Cinzel, serif;
       letter-spacing: 0.16em; margin-right: -0.16em; text-transform: uppercase; }
  .tagline { margin: 0 0 32px; text-align: center; color: var(--muted); font-style: italic; }
  a.game { display: block; margin-bottom: 14px; padding: 18px 20px; border: 1px solid var(--gold-dim);
           border-radius: 4px; background: var(--surface); color: inherit; text-decoration: none;
           transition: border-color 160ms ease, box-shadow 160ms ease; }
  a.game:hover, a.game:focus-visible { border-color: var(--gold); outline: none;
           box-shadow: 0 0 18px rgb(91 143 214 / 0.35); }
  .name { display: block; font: 500 1.15rem/1.3 Cinzel, serif; letter-spacing: 0.12em;
          text-transform: uppercase; color: var(--gold); }
  .about { display: block; color: var(--muted); }
  @media (prefers-reduced-motion: reduce) { a.game { transition: none; } }
</style>
</head>
<body>
<main>
  <h1>emmanems</h1>
  <p class="tagline">Daily puzzles</p>
  <a class="game" href="/doomdle/">
    <span class="name">Doomdle</span>
    <span class="about">A daily Magic: The Gathering puzzle: build the Doomsday pile that wins this turn.</span>
  </a>
  <a class="game" href="/emmadle/">
    <span class="name">Emmadle</span>
    <span class="about">A daily game from my Letterboxd reviews: which film did I rate higher?</span>
  </a>
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
