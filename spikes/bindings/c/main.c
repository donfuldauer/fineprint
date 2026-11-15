/* This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
 * If a copy of the MPL was not distributed with this file, You can obtain one at
 * https://mozilla.org/MPL/2.0/. */

/*
 * FP-005 (#6) spike C caller (REQ-NFR-002, REQ-NFR-003).
 *
 * Usage: spike_c <vectors.txt>
 *
 * Reads the shared fixed-input file (one "<name> <input-hex or -> <expected-hex>" per line),
 * hashes every input through fp_spike_sha256 from the cbindgen-generated header and prints
 * "<name> <lowercase hex>\n" per input to stdout. Exits nonzero on any API error, on a malformed
 * vectors file, or if a digest differs from the expected column.
 */

#if defined(_WIN32)
#define _CRT_SECURE_NO_WARNINGS 1
#include <fcntl.h>
#include <io.h>
#endif

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fineprint_spike.h"

#define MAX_LINE 8192
#define MAX_INPUT (MAX_LINE / 2)

static int hex_value(char c) {
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    return -1;
}

/* Decodes hex text into out; "-" is the empty input. Returns the byte count or -1. */
static long decode_hex(const char *text, uint8_t *out, size_t cap) {
    size_t n = strlen(text);
    size_t i;
    if (strcmp(text, "-") == 0) {
        return 0;
    }
    if (n % 2 != 0 || n / 2 > cap) {
        return -1;
    }
    for (i = 0; i < n; i += 2) {
        int hi = hex_value(text[i]);
        int lo = hex_value(text[i + 1]);
        if (hi < 0 || lo < 0) {
            return -1;
        }
        out[i / 2] = (uint8_t)((hi << 4) | lo);
    }
    return (long)(n / 2);
}

static void to_hex(const uint8_t *digest, char *text) {
    static const char digits[] = "0123456789abcdef";
    size_t i;
    for (i = 0; i < FP_SPIKE_SHA256_LEN; i++) {
        text[2 * i] = digits[digest[i] >> 4];
        text[2 * i + 1] = digits[digest[i] & 0x0f];
    }
    text[2 * FP_SPIKE_SHA256_LEN] = '\0';
}

/* Splits the next whitespace-separated field off *cursor (modifies the buffer). */
static char *next_field(char **cursor) {
    char *p = *cursor;
    char *start;
    while (*p == ' ' || *p == '\t' || *p == '\r' || *p == '\n') {
        p++;
    }
    if (*p == '\0') {
        *cursor = p;
        return NULL;
    }
    start = p;
    while (*p != '\0' && *p != ' ' && *p != '\t' && *p != '\r' && *p != '\n') {
        p++;
    }
    if (*p != '\0') {
        *p = '\0';
        p++;
    }
    *cursor = p;
    return start;
}

/* The API must reject bad arguments with a nonzero code (REQ-NFR-002). */
static int check_error_paths(void) {
    static const uint8_t abc[3] = {'a', 'b', 'c'};
    uint8_t out[FP_SPIKE_SHA256_LEN];
    if (fp_spike_sha256(abc, sizeof abc, NULL) == FP_SPIKE_OK) {
        fprintf(stderr, "null out was accepted\n");
        return 1;
    }
    if (fp_spike_sha256(NULL, 1, out) == FP_SPIKE_OK) {
        fprintf(stderr, "null data with len 1 was accepted\n");
        return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    static char line[MAX_LINE];
    static uint8_t input[MAX_INPUT];
    uint8_t digest[FP_SPIKE_SHA256_LEN];
    char hex[2 * FP_SPIKE_SHA256_LEN + 1];
    FILE *f;
    int count = 0;

#if defined(_WIN32)
    /* Keep "\n" as LF on Windows so every caller's output is byte-identical. */
    if (_setmode(_fileno(stdout), _O_BINARY) == -1) {
        fprintf(stderr, "cannot set stdout to binary mode\n");
        return 1;
    }
#endif

    if (argc != 2) {
        fprintf(stderr, "usage: %s <vectors.txt>\n", argv[0]);
        return 2;
    }
    if (check_error_paths() != 0) {
        return 1;
    }
    f = fopen(argv[1], "rb");
    if (f == NULL) {
        fprintf(stderr, "cannot open %s\n", argv[1]);
        return 1;
    }
    while (fgets(line, (int)sizeof line, f) != NULL) {
        char *cursor = line;
        char *name = next_field(&cursor);
        char *input_hex;
        char *expected;
        long len;
        int rc;
        if (name == NULL) {
            continue; /* blank line */
        }
        input_hex = next_field(&cursor);
        expected = next_field(&cursor);
        if (input_hex == NULL || expected == NULL || next_field(&cursor) != NULL) {
            fprintf(stderr, "malformed line for %s\n", name);
            fclose(f);
            return 1;
        }
        len = decode_hex(input_hex, input, sizeof input);
        if (len < 0) {
            fprintf(stderr, "bad input hex for %s\n", name);
            fclose(f);
            return 1;
        }
        rc = fp_spike_sha256(len == 0 ? NULL : input, (size_t)len, digest);
        if (rc != FP_SPIKE_OK) {
            fprintf(stderr, "fp_spike_sha256 failed for %s: %d\n", name, (int)rc);
            fclose(f);
            return 1;
        }
        to_hex(digest, hex);
        if (strcmp(hex, expected) != 0) {
            fprintf(stderr, "digest mismatch for %s: got %s want %s\n", name, hex, expected);
            fclose(f);
            return 1;
        }
        printf("%s %s\n", name, hex);
        count++;
    }
    if (ferror(f)) {
        fprintf(stderr, "read error on %s\n", argv[1]);
        fclose(f);
        return 1;
    }
    fclose(f);
    if (count == 0) {
        fprintf(stderr, "no vectors in %s\n", argv[1]);
        return 1;
    }
    return fflush(stdout) == 0 ? 0 : 1;
}
