// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike: minimal static HTTP server for the browser test (REQ-NFR-003).
// Serves spikes/bindings/vectors.txt, spikes/bindings/web/* and spikes/bindings/dist/web/*
// from 127.0.0.1 on an ephemeral port. Readiness is the resolved promise of startServer (no
// sleeps, R7). Standalone: `node web/server.mjs [port]` prints the base URL.

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const ALLOWED = ["/vectors.txt", "/web/", "/dist/web/"];
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".wasm": "application/wasm",
};

async function handle(req, res) {
  const path = decodeURIComponent(new URL(req.url ?? "/", "http://localhost").pathname);
  const allowed = ALLOWED.some((p) => (p.endsWith("/") ? path.startsWith(p) : path === p));
  const file = resolve(ROOT, `.${path}`);
  const type = TYPES[extname(file)];
  if (req.method !== "GET" || !allowed || !file.startsWith(ROOT.endsWith(sep) ? ROOT : ROOT + sep) || !type) {
    res.writeHead(404).end();
    return;
  }
  try {
    const body = await readFile(file);
    res.writeHead(200, { "content-type": type, "cache-control": "no-store" }).end(body);
  } catch {
    res.writeHead(404).end();
  }
}

/**
 * Starts the server and resolves once it is listening.
 * @param {number} [port] 0 for an ephemeral port
 * @returns {Promise<{ url: string, close: () => Promise<void> }>}
 */
export function startServer(port = 0) {
  const server = createServer((req, res) => {
    handle(req, res).catch(() => res.writeHead(500).end());
  });
  return new Promise((resolveStart, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      const address = server.address();
      const url = `http://127.0.0.1:${typeof address === "object" && address ? address.port : port}`;
      resolveStart({
        url,
        close: () => new Promise((done) => server.close(() => done())),
      });
    });
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { url } = await startServer(Number(process.argv[2] ?? 0));
  process.stdout.write(`${url}/web/index.html\n`);
}
