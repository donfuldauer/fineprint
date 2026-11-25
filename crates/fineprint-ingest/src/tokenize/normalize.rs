// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Steps 1 and 2 of the tokenizer: typographic normalization (REQ-ING-004(e)) and reminder-text
//! stripping (REQ-ING-004(a), CR 207.2a).

use std::collections::BTreeSet;

/// True for the Unicode noncharacters (U+FDD0..=U+FDEF and every code point ending in FFFE or
/// FFFF), which `TextTokenStream::new` refuses.
pub(super) fn is_noncharacter(c: char) -> bool {
    let cp = u32::from(c);
    (0xFDD0..=0xFDEF).contains(&cp) || cp & 0xFFFE == 0xFFFE
}

/// Maps one non-line-break character to its normalized form.
fn map_char(c: char) -> char {
    match c {
        // Tab and the Unicode space separators (Zs), plus the line and paragraph separators,
        // become an ASCII space; none of them is a paragraph break.
        '\t'
        | '\u{A0}'
        | '\u{1680}'
        | '\u{2000}'..='\u{200A}'
        | '\u{2028}'
        | '\u{2029}'
        | '\u{202F}'
        | '\u{205F}'
        | '\u{3000}' => ' ',
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        // Hyphen, non-breaking hyphen, figure dash, en dash, minus sign. The em dash U+2014 is
        // the structural separator and stays.
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2212}' => '-',
        c if is_noncharacter(c) => '\u{FFFD}',
        c => c,
    }
}

/// Normalizes typographic variants (REQ-ING-004(e)); see [`super::normalize_text`].
pub(super) fn normalize(input: &str) -> String {
    let s = input.trim_start_matches('\u{FEFF}');
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            trim_trailing_spaces(&mut out);
            out.push('\n');
        } else if c == '\n' {
            trim_trailing_spaces(&mut out);
            out.push('\n');
        } else {
            out.push(map_char(c));
        }
    }
    trim_trailing_spaces(&mut out);
    out
}

/// Removes ASCII spaces at the end of `out`.
fn trim_trailing_spaces(out: &mut String) {
    let len = out.trim_end_matches(' ').len();
    out.truncate(len);
}

/// Removes balanced top-level parenthesized spans from one normalized line (CR 207.2a), keeping
/// any span whose exact text is in `keep`. Unbalanced parentheses stay as literal text. When at
/// least one span is removed, runs of spaces collapse to one, a space before `.`, `,`, `;` or `:`
/// is dropped and the line is trimmed. Linear in the line length apart from one sort.
pub(super) fn strip_reminder(line: &str, keep: &BTreeSet<String>) -> String {
    // Every matched pair (open byte, end byte) via a stack; unmatched brackets stay literal.
    let mut stack = Vec::new();
    let mut pairs = Vec::new();
    for (i, c) in line.char_indices() {
        match c {
            '(' => stack.push(i),
            ')' => {
                if let Some(open) = stack.pop() {
                    pairs.push((open, i + 1));
                }
            }
            _ => {}
        }
    }
    if pairs.is_empty() {
        return line.to_string();
    }
    pairs.sort_unstable();
    let mut out = String::with_capacity(line.len());
    let mut pos = 0;
    let mut removed = false;
    for (open, end) in pairs {
        if open < pos {
            continue; // nested inside an outer pair already handled
        }
        if keep.contains(&line[open..end]) {
            out.push_str(&line[pos..end]);
        } else {
            out.push_str(&line[pos..open]);
            removed = true;
        }
        pos = end;
    }
    out.push_str(&line[pos..]);
    if removed { tidy(&out) } else { out }
}

/// Collapses space runs, drops a space before `.`, `,`, `;` and `:`, and trims.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' if out.ends_with(' ') => {}
            '.' | ',' | ';' | ':' => {
                if out.ends_with(' ') {
                    out.pop();
                }
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.trim_matches(' ').to_string()
}
