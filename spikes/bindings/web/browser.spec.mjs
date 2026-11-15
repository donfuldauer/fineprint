// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike: the wasm build hashes identically in headless Chromium, Firefox and WebKit
// (REQ-NFR-002, REQ-NFR-003). No sleeps or wall-clock assertions (R7): the test waits on the
// server's listening promise and the page's `window.spikeResult` promise.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { startServer } from "./server.mjs";
import { parseVectors } from "./vectors.mjs";

const vectorsText = readFileSync(fileURLToPath(new URL("../vectors.txt", import.meta.url)), "utf8");
const expectedOutput = parseVectors(vectorsText)
  .map((v) => `${v.name} ${v.expected}\n`)
  .join("");
const resultsDir = fileURLToPath(new URL("../dist/results/", import.meta.url));

let server;

test.beforeAll(async () => {
  server = await startServer();
});

test.afterAll(async () => {
  await server?.close();
});

test("REQ-NFR-002 REQ-NFR-003: wasm sha256 matches vectors.txt in the browser", async ({
  page,
  browserName,
}) => {
  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(String(e)));
  await page.goto(`${server.url}/web/index.html`);
  await page.waitForFunction(() => window.spikeResult !== undefined);
  const output = await page.evaluate(() => window.spikeResult);
  expect(pageErrors).toEqual([]);
  expect(output).toBe(expectedOutput);
  mkdirSync(resultsDir, { recursive: true });
  writeFileSync(`${resultsDir}wasm-browser-${browserName}.txt`, output);
});
