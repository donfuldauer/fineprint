// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike: browser caller (REQ-NFR-002, REQ-NFR-003). Initialises the `--target web`
// wasm build, hashes every input of the shared vectors.txt and publishes the caller output as
// the promise `window.spikeResult` (the explicit signal the Playwright test waits on, R7).

import init, { sha256_hex } from "../dist/web/fineprint_spike_bindings.js";
import { runVectors } from "./vectors.mjs";

async function run() {
  await init({
    module_or_path: new URL("../dist/web/fineprint_spike_bindings_bg.wasm", import.meta.url),
  });
  const response = await fetch(new URL("../vectors.txt", import.meta.url));
  if (!response.ok) {
    throw new Error(`vectors.txt: HTTP ${response.status}`);
  }
  const out = runVectors(await response.text(), sha256_hex);
  document.getElementById("result").textContent = out;
  return out;
}

window.spikeResult = run();
