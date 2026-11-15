// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike: type-checks a TypeScript consumer against the wasm-bindgen generated
// .d.ts files of both builds (REQ-NFR-002 "generated TypeScript type definitions").
// Checked with `tsc --noEmit -p tsconfig.json`; never executed.

import init, { sha256_hex, type InitOutput } from "../dist/web/fineprint_spike_bindings.js";
import * as nodeBinding from "../dist/node/fineprint_spike_bindings.js";

export async function hashInBrowser(data: Uint8Array): Promise<string> {
  const exports: InitOutput = await init();
  void exports.memory;
  const hex: string = sha256_hex(data);
  return hex;
}

export function hashInNode(data: Uint8Array): string {
  return nodeBinding.sha256_hex(data);
}

export function typesAreStrict(): void {
  // @ts-expect-error a string is not a byte array; fails the check if the types were `any`.
  sha256_hex("abc");
  // @ts-expect-error the result is a string, not a number.
  const n: number = nodeBinding.sha256_hex(new Uint8Array(0));
  void n;
}
