// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike: parser for the shared fixed-input file vectors.txt (REQ-NFR-003).
// Used unchanged by the Node.js caller, the browser page and the Playwright test.
// Each non-blank line is "<name> <input-hex or -> <expected-sha256-hex>".

/**
 * Decodes lowercase or uppercase hex; "-" is the empty input.
 * @param {string} text
 * @returns {Uint8Array}
 */
export function decodeHex(text) {
  if (text === "-") {
    return new Uint8Array(0);
  }
  if (text.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(text)) {
    throw new Error(`bad hex input: ${text}`);
  }
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = Number.parseInt(text.slice(2 * i, 2 * i + 2), 16);
  }
  return out;
}

/**
 * Parses vectors.txt.
 * @param {string} text
 * @returns {{ name: string, input: Uint8Array, expected: string }[]}
 */
export function parseVectors(text) {
  const vectors = [];
  for (const line of text.split("\n")) {
    const fields = line.trim().split(/\s+/).filter((f) => f.length > 0);
    if (fields.length === 0) {
      continue;
    }
    if (fields.length !== 3) {
      throw new Error(`malformed vectors line: ${line}`);
    }
    vectors.push({ name: fields[0], input: decodeHex(fields[1]), expected: fields[2] });
  }
  if (vectors.length === 0) {
    throw new Error("no vectors");
  }
  return vectors;
}

/**
 * Hashes every vector with `sha256Hex` and returns the caller output: one
 * "<name> <hex>\n" line per input. Throws if any digest differs from the expected column.
 * @param {string} text contents of vectors.txt
 * @param {(data: Uint8Array) => string} sha256Hex
 * @returns {string}
 */
export function runVectors(text, sha256Hex) {
  let out = "";
  for (const v of parseVectors(text)) {
    const hex = sha256Hex(v.input);
    if (hex !== v.expected) {
      throw new Error(`digest mismatch for ${v.name}: got ${hex} want ${v.expected}`);
    }
    out += `${v.name} ${hex}\n`;
  }
  return out;
}
