// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for REQ-LAYER-002 item 5: dependency loops (CR 613.8b) and recomputing the
//! dependency relation after each application (CR 613.8c). SRS 6.2: a loop is a strongly
//! connected component of the dependency graph with no incoming edge from outside it; only its
//! earliest-timestamped member is applied, then dependencies are recomputed. Black-box: written
//! from the work-item brief and SRS 6.2 only. Every scenario also checks
//! `evaluate == evaluate_with_order(..).0` (item 9).
//!
//! The loops are built from layer-4 effects that each add a marker subtype and apply only while
//! another marker is absent, so applying one changes what another applies to (CR 613.8a (b)).

#[path = "spec_layers_ordering_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_core::layers::{ContinuousEffect, Selection};
use fineprint_dsl::ability::Layer;
use support::*;

/// A layer-4 non-CDA effect: applies while the object lacks subtype `unless`, adds `mark`.
fn marker(t: u64, unless: &str, mark: &str) -> ContinuousEffect {
    eff(
        Layer::L4,
        t,
        sel(not(has_subtype(unless))),
        add_subtype(mark),
    )
}

fn subtypes<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    set(items)
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: a two-effect loop (each one's application stops the other
/// from applying). The earliest member is applied (Selection::Loop); applying it breaks the
/// loop, so the other is then chosen by the normal rules (Timestamp) and no longer applies.
/// Both timestamp orders; swapping the timestamps swaps the result.
#[test]
fn two_effect_loop_in_timestamp_order_both_orders() {
    for (ta, tb) in [(1, 2), (2, 1)] {
        let r = registry(&[
            (1, marker(ta, "MarkB", "MarkA")),
            (2, marker(tb, "MarkA", "MarkB")),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
        let (first, second, kept) = if ta < tb {
            (1, 2, "MarkA")
        } else {
            (2, 1, "MarkB")
        };
        assert_eq!(ev.characteristics.subtypes, subtypes([kept]));
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![
                fx(Layer::L4, first, Selection::Loop, true),
                fx(Layer::L4, second, Selection::Timestamp, false),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: a three-effect cycle (E2 depends on E1, E3 on E2, E1 on
/// E3). For every assignment of timestamps, the earliest member k is applied first
/// (Selection::Loop); that stops its successor from applying and so breaks the cycle; the two
/// others are then chosen by timestamp. The result keeps the marks of k and of k's predecessor.
#[test]
fn three_effect_loop_every_timestamp_permutation() {
    let marks = ["Mark1", "Mark2", "Mark3"];
    let perms: [[u64; 3]; 6] = [
        [1, 2, 3],
        [1, 3, 2],
        [2, 1, 3],
        [2, 3, 1],
        [3, 1, 2],
        [3, 2, 1],
    ];
    for t in perms {
        // Effect i (id i+1) applies unless the mark of effect i-1 (cyclically) is present.
        let r = registry(&[
            (1, marker(t[0], marks[2], marks[0])),
            (2, marker(t[1], marks[0], marks[1])),
            (3, marker(t[2], marks[1], marks[2])),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
        let k = (0..3).min_by_key(|&i| t[i]).unwrap();
        let succ = (k + 1) % 3;
        let pred = (k + 2) % 3;
        let id = |i: usize| u64::try_from(i + 1).unwrap();
        let mut rest = [succ, pred];
        rest.sort_by_key(|&i| t[i]);
        let expected: Vec<_> = std::iter::once(fx(Layer::L4, id(k), Selection::Loop, true))
            .chain(
                rest.iter()
                    .map(|&i| fx(Layer::L4, id(i), Selection::Timestamp, i == pred)),
            )
            .collect();
        assert_eq!(steps_in(&rec, Layer::L4), expected, "timestamps {t:?}");
        assert_eq!(
            ev.characteristics.subtypes,
            [marks[k], marks[pred]]
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<BTreeSet<_>>(),
            "timestamps {t:?}"
        );
    }
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: recomputation after each application can change the
/// order. E1 (ts 1) and E2 (ts 2) form a loop; E3 (ts 3) depends on E2. E1 is applied first
/// (Loop). Recomputed against the new state, E2 now depends on E3 (E3 would make it apply
/// again) and E3 depends on nothing, so E3 applies before E2 (Dependency) and E2 then applies
/// (Timestamp). Timestamp order without recomputation would have given E1, E2, E3 and left E2
/// unapplied.
#[test]
fn recomputation_after_loop_member_changes_order() {
    let r = registry(&[
        (1, marker(1, "Mark2", "Mark1")),
        (
            2,
            eff(
                Layer::L4,
                2,
                sel(or(vec![not(has_subtype("Mark1")), has_subtype("Mark3")])),
                add_subtype("Mark2"),
            ),
        ),
        (3, marker(3, "Mark2", "Mark3")),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
    assert_eq!(
        ev.characteristics.subtypes,
        subtypes(["Mark1", "Mark2", "Mark3"])
    );
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 1, Selection::Loop, true),
            fx(Layer::L4, 3, Selection::Dependency, true),
            fx(Layer::L4, 2, Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: the same three effects with the loop members' timestamps
/// swapped: E2 is now the earliest loop member and is applied first (Loop); that stops both E1
/// and E3 from applying, nothing depends on anything any more, and they follow by timestamp.
#[test]
fn recomputation_after_loop_member_other_timestamp_order() {
    let r = registry(&[
        (1, marker(2, "Mark2", "Mark1")),
        (
            2,
            eff(
                Layer::L4,
                1,
                sel(or(vec![not(has_subtype("Mark1")), has_subtype("Mark3")])),
                add_subtype("Mark2"),
            ),
        ),
        (3, marker(3, "Mark2", "Mark3")),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
    assert_eq!(ev.characteristics.subtypes, subtypes(["Mark2"]));
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 2, Selection::Loop, true),
            fx(Layer::L4, 1, Selection::Timestamp, false),
            fx(Layer::L4, 3, Selection::Timestamp, false),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8b: a three-effect loop (ts 2, 3, 4) and an effect outside it with the
/// earliest timestamp (ts 1) that depends on two loop members throughout (it applies to objects
/// with exactly one of their marks). The outside effect is not part of the loop (its incoming
/// dependency makes it no loop), so the loop's earliest member goes first (Loop), the remaining
/// members follow in timestamp order, and the outside effect waits until after all of them.
#[test]
fn loop_with_outside_dependent_waits_for_all_members() {
    let r = registry(&[
        (
            10,
            eff(
                Layer::L4,
                1,
                sel(xor(has_subtype("Mark1"), has_subtype("Mark3"))),
                add_subtype("Waited"),
            ),
        ),
        (1, marker(2, "Mark3", "Mark1")),
        (2, marker(3, "Mark1", "Mark2")),
        (3, marker(4, "Mark2", "Mark3")),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
    assert_eq!(ev.characteristics.subtypes, subtypes(["Mark1", "Mark3"]));
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 1, Selection::Loop, true),
            fx(Layer::L4, 2, Selection::Dependency, false),
            fx(Layer::L4, 3, Selection::Dependency, true),
            fx(Layer::L4, 10, Selection::Timestamp, false),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8b: a two-effect loop (ts 2, 3) and a later outside effect (ts 4) that
/// depends on both members: the loop members go first in timestamp order (the first as Loop),
/// the outside effect after them.
#[test]
fn two_effect_loop_then_later_outside_dependent() {
    let r = registry(&[
        (1, marker(2, "MarkB", "MarkA")),
        (2, marker(3, "MarkA", "MarkB")),
        (
            10,
            eff(
                Layer::L4,
                4,
                sel(xor(has_subtype("MarkA"), has_subtype("MarkB"))),
                add_subtype("Waited"),
            ),
        ),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
    assert_eq!(ev.characteristics.subtypes, subtypes(["MarkA", "Waited"]));
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 1, Selection::Loop, true),
            fx(Layer::L4, 2, Selection::Timestamp, false),
            fx(Layer::L4, 10, Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: a two-effect loop (ts 2, 3) and an earlier outside effect
/// (ts 1) that depends on both members. With no candidate, the loop (the component with no
/// incoming edge) is found, not the earliest effect overall: the loop's earliest member goes
/// first (Loop). That stops the other member from applying, so it no longer changes anything
/// and the outside effect no longer depends on it (613.8c): the outside effect is then the
/// earliest candidate and applies before the remaining member (SRS 6.2 step 3).
#[test]
fn two_effect_loop_with_earlier_outside_dependent_recomputes() {
    let r = registry(&[
        (
            10,
            eff(
                Layer::L4,
                1,
                sel(xor(has_subtype("MarkA"), has_subtype("MarkB"))),
                add_subtype("Waited"),
            ),
        ),
        (1, marker(2, "MarkB", "MarkA")),
        (2, marker(3, "MarkA", "MarkB")),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
    assert_eq!(ev.characteristics.subtypes, subtypes(["MarkA", "Waited"]));
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 1, Selection::Loop, true),
            fx(Layer::L4, 10, Selection::Timestamp, true),
            fx(Layer::L4, 2, Selection::Timestamp, false),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8b, 613.3, 613.8a: two CDAs can form a loop (613.8a (c): both from
/// CDAs) ahead of a non-CDA effect of the layer. The first loop member is recorded as Loop;
/// the other CDA, chosen while the non-CDA step is unapplied, as CdaFirst; the non-CDA effect
/// last, as Timestamp.
#[test]
fn cda_loop_before_non_cda() {
    let cda_marker = |t: u64, unless: &str, mark: &str| {
        cda(
            Layer::L4,
            t,
            sel(not(has_subtype(unless))),
            add_subtype(mark),
        )
    };
    let r = registry(&[
        (1, cda_marker(2, "MarkB", "MarkA")),
        (2, cda_marker(3, "MarkA", "MarkB")),
        (3, eff(Layer::L4, 1, subject(), add_subtype("KindN"))),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
    assert_eq!(ev.characteristics.subtypes, subtypes(["KindN", "MarkA"]));
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 1, Selection::Loop, true),
            fx(Layer::L4, 2, Selection::CdaFirst, false),
            fx(Layer::L4, 3, Selection::Timestamp, true),
        ]
    );
}
