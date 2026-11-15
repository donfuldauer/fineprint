// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike: Node.js caller of the wasm-bindgen build (REQ-NFR-002, REQ-NFR-003).
// Usage: node web/node.mjs [vectors.txt]   (run from spikes/bindings)
// Loads the `--target nodejs` output from dist/node (CommonJS, so through createRequire),
// prints "<name> <hex>" per input and exits nonzero on any mismatch or error.

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { runVectors } from "./vectors.mjs";

const require = createRequire(import.meta.url);
const binding = require("../dist/node/fineprint_spike_bindings.js");

const vectorsPath = process.argv[2] ?? fileURLToPath(new URL("../vectors.txt", import.meta.url));
process.stdout.write(runVectors(readFileSync(vectorsPath, "utf8"), binding.sha256_hex));
