//! The shipped-path blockers: every supply the run holds reaches the turn's facts (meds and salt
//! were left out by a hand-written key list), no composed line ever carries a `PLACEHOLDER-`
//! stand-in, and the sampling seed tells apart turns that differ only in phase or action.

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepMessages;
use etv_core::model::{GMProfile, GamePhase, IntentAction, PlayerIntent};
use etv_core::narrator::{
    build_narration_context_from_turn, compute_applied_turn_deltas, evaluate_turn_matters,
    narration_call_seed, resolve_fallback_line_with_mode, CheckerContext, ComposedFallbackComposer,
    FallbackComposerError, FallbackPhraseTable, NarrationContext, NarratorLength,
    PreTurnObservation,
};
use etv_core::worldgen::create_new_run_with_content;

/// A camp turn on which the party spends `spent` of `key` and nothing else changes.
fn turn_spending(key: &str, spent: i32) -> NarrationContext {
    let mut state = create_new_run_with_content(42, embedded_content_pack());
    let pre = PreTurnObservation::capture(GamePhase::Camp, &state);
    let before = state.supplies.get(key);
    assert!(before >= spent, "the new run must hold {spent} {key}");
    state.supplies.set(key, before - spent);
    let mut msgs = StepMessages::default();
    msgs.outcome_deltas.insert(key.to_string(), -spent);
    let flags = evaluate_turn_matters(&pre, &PlayerIntent::new(IntentAction::Rest), &msgs, &state);
    build_narration_context_from_turn(&pre, &state, GamePhase::Camp, "", "", &msgs, flags)
}

#[test]
fn every_supply_the_run_holds_is_in_the_applied_deltas() {
    let mut state = create_new_run_with_content(42, embedded_content_pack());
    let pre = PreTurnObservation::capture(GamePhase::Camp, &state);
    // Spend one of every supply the run holds above zero.
    let keys: Vec<String> = state
        .supplies
        .items
        .iter()
        .filter(|(_, &v)| v > 0)
        .map(|(k, _)| k.clone())
        .collect();
    for key in &keys {
        let v = state.supplies.get(key);
        state.supplies.set(key, v - 1);
    }
    let applied = compute_applied_turn_deltas(&pre, &state);
    for key in &keys {
        let shown = if key == "lantern_oil" {
            "oil"
        } else {
            key.as_str()
        };
        assert_eq!(
            applied.get(shown),
            Some(&-1),
            "{key} missing from {applied:?}"
        );
    }
    assert!(applied.contains_key("meds") && applied.contains_key("salt"));
}

#[test]
fn a_turn_that_spends_meds_shows_it_in_the_facts_the_fallback_and_the_allowed_set() {
    for key in ["meds", "salt"] {
        let ctx = turn_spending(key, 1);
        assert_eq!(
            ctx.shown_deltas().get(key),
            Some(&-1),
            "{key} not in the shown deltas"
        );

        let checker = CheckerContext::from_narration_context(&ctx, NarratorLength::Standard);
        assert!(
            checker.allowed_resources.iter().any(|k| k == key),
            "{key} not in allowed_resources {:?}",
            checker.allowed_resources
        );

        let table = FallbackPhraseTable::embedded();
        for release in [false, true] {
            let line = resolve_fallback_line_with_mode(&ctx, Some(&table), release);
            assert!(!line.contains("PLACEHOLDER"), "placeholder in {line:?}");
            // The approved phrases (2026-11-14) are in the line.
            let phrase = table
                .phrase(key, "neg")
                .unwrap_or_else(|| panic!("the shipped table has no {key} phrase"));
            let expected = if key == "meds" {
                "uses medicine"
            } else {
                "uses salt"
            };
            assert_eq!(phrase, expected);
            assert!(
                line.contains(expected),
                "{line:?} lacks the {key} phrase {expected:?}"
            );
        }
    }
}

#[test]
fn a_turn_that_gains_meds_or_salt_says_so_in_the_fallback() {
    for (key, expected) in [("meds", "gains medicine"), ("salt", "gains salt")] {
        let mut state = create_new_run_with_content(42, embedded_content_pack());
        let pre = PreTurnObservation::capture(GamePhase::Camp, &state);
        let before = state.supplies.get(key);
        state.supplies.set(key, before + 1);
        let mut msgs = StepMessages::default();
        msgs.outcome_deltas.insert(key.to_string(), 1);
        let flags =
            evaluate_turn_matters(&pre, &PlayerIntent::new(IntentAction::Rest), &msgs, &state);
        let ctx =
            build_narration_context_from_turn(&pre, &state, GamePhase::Camp, "", "", &msgs, flags);
        assert_eq!(ctx.shown_deltas().get(key), Some(&1));
        let table = FallbackPhraseTable::embedded();
        for release in [false, true] {
            let line = resolve_fallback_line_with_mode(&ctx, Some(&table), release);
            assert!(line.contains(expected), "{line:?} lacks {expected:?}");
        }
    }
}

#[test]
fn no_composed_line_carries_a_placeholder_for_any_supply_key() {
    let table = FallbackPhraseTable::embedded();
    let composer = ComposedFallbackComposer::try_start_with_mode(Some(&table), true)
        .expect("the shipped table starts in a release build");
    let state = create_new_run_with_content(42, embedded_content_pack());
    let mut keys: Vec<String> = state.supplies.items.keys().cloned().collect();
    keys.push("a_key_no_table_knows".to_string());
    for key in &keys {
        for delta in [-1, 1] {
            let line = composer.compose_from_effects(&[(key.as_str(), delta)]);
            assert!(!line.contains("PLACEHOLDER"), "{key} {delta}: {line:?}");
        }
    }
}

#[test]
fn a_table_without_a_real_neutral_phrase_never_composes_a_placeholder() {
    // A custom table with no "none" entry, and a debug table whose "none" entry is only a
    // stand-in. Before, a turn with nothing phrased composed "The party PLACEHOLDER-none-neutral."
    let no_none = FallbackPhraseTable::from_json_str(
        r#"{"schema_version":1,"phrases":{"food":{"neg":"uses food","pos":"gains food"}}}"#,
    )
    .unwrap();
    assert!(!no_none.has_placeholders());
    let mut stand_in = FallbackPhraseTable::embedded();
    stand_in.phrases.get_mut("none").unwrap().insert(
        "neutral".to_string(),
        "PLACEHOLDER-none-neutral".to_string(),
    );
    for table in [&no_none, &stand_in] {
        for release in [false, true] {
            let refused = ComposedFallbackComposer::try_start_with_mode(Some(table), release);
            let expected = if release && table.has_placeholders() {
                FallbackComposerError::PlaceholdersInReleaseBuild
            } else {
                FallbackComposerError::NeutralPhraseMissing
            };
            assert_eq!(refused.unwrap_err(), expected, "release {release}");
        }
    }

    // The caller then shows the engine's own line, which carries no stand-in either.
    let nothing_phrased = turn_spending("water", 1);
    for table in [&no_none, &stand_in] {
        for release in [false, true] {
            let line = resolve_fallback_line_with_mode(&nothing_phrased, Some(table), release);
            assert_eq!(line, nothing_phrased.fallback_line());
            assert!(!line.contains("PLACEHOLDER"), "{line:?}");
        }
    }

    // The shipped table has a real neutral phrase and still composes it.
    let shipped = FallbackPhraseTable::embedded();
    let composer = ComposedFallbackComposer::try_start_with_mode(Some(&shipped), true).unwrap();
    let neutral = shipped.phrase("none", "neutral").unwrap();
    assert_eq!(
        composer.compose_from_effects(&[]),
        format!("{} {neutral}.", shipped.subject)
    );
}

#[test]
fn turns_that_differ_only_in_phase_or_action_get_different_seeds() {
    let base = turn_spending("food", 1).with_turn_index(5);
    let seed = |c: &NarrationContext| {
        narration_call_seed(c, GMProfile::Fireside, NarratorLength::Brief, false)
    };
    let rest = base
        .clone()
        .with_event_facts(String::new(), Vec::new(), "REST".to_string());
    let hunt = base
        .clone()
        .with_event_facts(String::new(), Vec::new(), "HUNT".to_string());
    assert_ne!(seed(&rest), seed(&hunt), "the action is in the seed");

    let mut state = create_new_run_with_content(42, embedded_content_pack());
    let pre = PreTurnObservation::capture(GamePhase::Camp, &state);
    let food = state.supplies.get("food");
    state.supplies.set("food", food - 1);
    let msgs = StepMessages::default();
    let flags = evaluate_turn_matters(&pre, &PlayerIntent::new(IntentAction::Rest), &msgs, &state);
    let at_camp =
        build_narration_context_from_turn(&pre, &state, GamePhase::Camp, "", "", &msgs, flags)
            .with_turn_index(5);
    let on_route =
        build_narration_context_from_turn(&pre, &state, GamePhase::Route, "", "", &msgs, flags)
            .with_turn_index(5);
    assert_ne!(seed(&at_camp), seed(&on_route), "the phase is in the seed");
    // The same turn asks for the same seed every time.
    assert_eq!(seed(&rest), seed(&rest.clone()));
}
