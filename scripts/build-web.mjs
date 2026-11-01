#!/usr/bin/env node
/* Krovi website tooling (owner: web agent). Used by scripts/build-web.ps1; also runnable directly.

   node scripts/build-web.mjs sync            inject the shared header/footer partials into every page
   node scripts/build-web.mjs check [--external] [--no-demo]
                                              guards: marketing-site marker, link/anchor resolution,
                                              CSP hygiene, zero third-party resource requests
   node scripts/build-web.mjs serve [port]    static server that mimics vercel.json (cleanUrls + headers)

   No dependencies beyond Node's standard library. */
import { readFileSync, writeFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join, extname, resolve, posix } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const web = join(root, "web");
const MARKER = "<!-- krovi-marketing-site -->";

const pages = () => readdirSync(web).filter((f) => f.endsWith(".html")).sort();
const read = (p) => readFileSync(p, "utf8");

/* ------------------------------------------------------------------ sync */
function sync() {
  const header = read(join(here, "build-web.header.html")).trimEnd();
  const footer = read(join(here, "build-web.footer.html")).trimEnd();
  const currentFor = { compare: "compare", docs: "docs", download: "download" };
  let changed = 0;
  for (const f of pages()) {
    const p = join(web, f);
    let html = read(p);
    const page = (html.match(/<body[^>]*data-page="([^"]+)"/) || [])[1] || "";
    let h = header;
    const nav = currentFor[page];
    if (nav) h = h.replace(new RegExp(`(<a [^>]*data-nav="${nav}")`, "g"), '$1 aria-current="page"');
    const next = html
      .replace(/<!--@header-->[\s\S]*?<!--\/@header-->/, () => `<!--@header-->\n${h}\n<!--/@header-->`)
      .replace(/<!--@footer-->[\s\S]*?<!--\/@footer-->/, () => `<!--@footer-->\n${footer}\n<!--/@footer-->`);
    if (next !== html) { writeFileSync(p, next); changed++; }
  }
  console.log(`sync: ${pages().length} pages, ${changed} updated`);
}

/* ----------------------------------------------------------------- check */
async function check(flags) {
  const errors = [];
  const warns = [];
  const err = (m) => errors.push(m);
  const warn = (m) => warns.push(m);
  const demoDir = join(web, "demo");
  const demoBuilt = existsSync(join(demoDir, "index.html"));

  // 1. the guard: the site root must be the marketing page, never the Vite bundle
  const indexPath = join(web, "index.html");
  if (!existsSync(indexPath)) err("web/index.html is missing");
  else {
    const idx = read(indexPath);
    if (!idx.includes(MARKER)) err(`web/index.html does not contain the marker ${MARKER}. Refusing: it may be the app bundle.`);
    if (/<div id="app"><\/div>/.test(idx) || /type="module"[^>]*src="[^"]*assets\//.test(idx))
      err("web/index.html looks like the Vite app bundle, not the marketing page");
    if (!/<h1[\s>]/.test(idx)) err("web/index.html has no <h1>");
  }
  if (!demoBuilt) (flags.has("--no-demo") ? warn : err)("web/demo/index.html is missing: run scripts/build-web.ps1 to build the demo");
  else {
    const d = read(join(demoDir, "index.html"));
    if (!/\/demo\/assets\//.test(d)) err("web/demo/index.html does not reference /demo/assets/ (was it built with --base=/demo/?)");
  }

  // 2. per-page checks
  const ids = {};
  const external = new Map();
  const htmls = {};
  for (const f of pages()) htmls[f] = read(join(web, f));
  for (const [f, html] of Object.entries(htmls)) {
    const set = new Set();
    for (const m of html.matchAll(/\sid="([^"]+)"/g)) {
      if (set.has(m[1])) err(`${f}: duplicate id "${m[1]}"`);
      set.add(m[1]);
    }
    ids[f] = set;
    if (!html.includes(MARKER)) err(`${f}: missing marketing marker`);
    if (!/<html lang="/.test(html)) err(`${f}: <html> needs a lang`);
    if (!/<title>[^<]{3,}<\/title>/.test(html)) err(`${f}: missing <title>`);
    if (!/<meta name="description" content="[^"]{20,}"/.test(html) ) err(`${f}: missing meta description`);
    if (f !== "404.html" && (html.match(/<h1[\s>]/g) || []).length !== 1) err(`${f}: needs exactly one <h1>`);
    if (/\sstyle="/.test(html)) err(`${f}: inline style attribute (blocked by the strict CSP)`);
    if (/<style[\s>]/.test(html)) err(`${f}: inline <style> (blocked by the strict CSP)`);
    for (const m of html.matchAll(/<script(?![^>]*\bsrc=)([^>]*)>/g))
      if (!/application\/ld\+json/.test(m[1])) err(`${f}: inline <script> (blocked by the strict CSP)`);
    if (/@header-->\s*<!--\/@header-->/.test(html)) err(`${f}: header partial empty: run "sync"`);
    if (/\bon[a-z]+="/.test(html.replace(/<!--[\s\S]*?-->/g, ""))) err(`${f}: inline event handler`);
    if (/<img\b(?![^>]*\balt=)/.test(html)) err(`${f}: <img> without alt`);
  }

  const resolveInternal = (fromFile, url) => {
    let [path, hash = ""] = url.split("#");
    path = path.split("?")[0];
    let target;
    if (path === "") target = fromFile;
    else {
      const abs = path.startsWith("/") ? path : posix.join("/", path);
      const rel = abs.replace(/^\//, "");
      if (rel === "" ) target = "index.html";
      else if (rel.startsWith("demo")) return { demo: true, hash };
      else if (existsSync(join(web, rel)) && statSync(join(web, rel)).isFile()) target = rel;
      else if (existsSync(join(web, rel + ".html"))) target = rel + ".html";
      else return { missing: true };
    }
    return { target, hash };
  };

  for (const [f, html] of Object.entries(htmls)) {
    const clean = html.replace(/<!--[\s\S]*?-->/g, "");
    for (const m of clean.matchAll(/\s(href|src|data-src)="([^"]*)"/g)) {
      const [, attr, url] = m;
      if (url === "" || url === "#") { err(`${f}: dead ${attr}="${url}"`); continue; }
      if (/^(mailto:|tel:|javascript:)/i.test(url)) { err(`${f}: ${url}`); continue; }
      if (/^https?:\/\//i.test(url)) {
        const host = new URL(url).host;
        const tag = (clean.slice(0, m.index).match(/<(\w+)[^<]*$/) || [])[1];
        if (attr !== "href" || tag !== "a") err(`${f}: third-party resource ${attr}="${url}" (${tag})`);
        if (!/^(github\.com|nodejs\.org|rustup\.rs|winlibs\.com|v2\.tauri\.app)$/.test(host)) err(`${f}: unexpected external host ${host}`);
        external.set(url, f);
        continue;
      }
      if (url.startsWith("data:")) continue;
      const r = resolveInternal(f, url);
      if (r.missing) { err(`${f}: broken link ${url}`); continue; }
      if (r.demo) { if (!demoBuilt && !flags.has("--no-demo")) err(`${f}: ${url} needs the demo build`); continue; }
      if (r.hash && !ids[r.target]?.has(decodeURIComponent(r.hash))) err(`${f}: ${url} -> no id "${r.hash}" in ${r.target}`);
    }
    // sprite references
    for (const m of clean.matchAll(/<use href="#([^"]+)"/g)) if (!ids[f]?.has(m[1])) err(`${f}: <use> references missing symbol #${m[1]}`);
  }

  // 3. optional: really request the external links (needs network; run before releasing)
  if (flags.has("--external")) {
    for (const [url, from] of external) {
      try {
        const base = url.split("#")[0];
        let res = await fetch(base, { method: "HEAD", redirect: "follow" });
        if (res.status === 405 || res.status === 403) res = await fetch(base, { method: "GET", redirect: "follow" });
        if (res.status >= 400) err(`${from}: external link ${url} -> HTTP ${res.status}`);
      } catch (e) { warn(`${from}: could not reach ${url} (${e.message})`); }
    }
  }

  warns.forEach((w) => console.warn("warn : " + w));
  if (errors.length) {
    errors.forEach((e) => console.error("FAIL : " + e));
    console.error(`\ncheck: ${errors.length} problem(s)`);
    process.exit(1);
  }
  console.log(`check: OK  (${Object.keys(htmls).length} pages, ${external.size} external links${flags.has("--external") ? ", all reachable" : ""})`);
}

/* ----------------------------------------------------------------- serve */
const TYPES = { ".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8", ".json": "application/json; charset=utf-8", ".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg", ".ico": "image/x-icon", ".txt": "text/plain; charset=utf-8", ".woff2": "font/woff2", ".webmanifest": "application/manifest+json", ".xml": "application/xml" };

function serve(port) {
  const cfg = JSON.parse(read(join(web, "vercel.json")));
  const rules = (cfg.headers || []).map((h) => ({ re: new RegExp("^" + h.source + "$"), headers: h.headers }));
  const server = createServer((req, res) => {
    let pathname = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const send = (file, status = 200) => {
      const body = readFileSync(file);
      const h = { "Content-Type": TYPES[extname(file)] || "application/octet-stream", "Cache-Control": "no-store" };
      for (const r of rules) if (r.re.test(pathname)) for (const { key, value } of r.headers) if (key.toLowerCase() !== "cache-control") h[key] = value;
      res.writeHead(status, h);
      res.end(req.method === "HEAD" ? undefined : body);
    };
    const inside = (p) => resolve(p).startsWith(web);
    for (const rw of cfg.rewrites || []) if (new RegExp("^" + rw.source + "$").test(pathname)) pathname = rw.destination;
    let file = join(web, pathname);
    if (!inside(file)) { res.writeHead(403); return res.end("Forbidden"); }
    if (cfg.cleanUrls && pathname.endsWith(".html")) { // vercel redirects /x.html -> /x
      res.writeHead(308, { Location: pathname.replace(/\.html$/, "") || "/" }); return res.end();
    }
    if (existsSync(file) && statSync(file).isDirectory()) file = join(file, "index.html");
    if (existsSync(file) && statSync(file).isFile()) return send(file);
    if (existsSync(file + ".html")) return send(file + ".html");
    const nf = join(web, "404.html");
    return existsSync(nf) ? send(nf, 404) : (res.writeHead(404), res.end("Not found"));
  });
  server.listen(port, () => console.log(`serving web/ at http://localhost:${port}  (vercel.json headers + cleanUrls)`));
}

/* ------------------------------------------------------------------ main */
const [cmd, ...rest] = process.argv.slice(2);
const flags = new Set(rest.filter((a) => a.startsWith("--")));
if (cmd === "sync") sync();
else if (cmd === "check") await check(flags);
else if (cmd === "serve") serve(parseInt(rest.find((a) => /^\d+$/.test(a)) || "5500", 10));
else { console.error("usage: build-web.mjs <sync|check [--external] [--no-demo]|serve [port]>"); process.exit(2); }
