//! The narrator is told what is actually happening: the event's title and its own text, the
//! choices on offer or the choice taken, and the camp action. The two calls of a turn differ in
//! substance: the scene call gets the situation, the outcome call also gets what resulted.
//!
//! No model is called anywhere in this file.

use etv_core::content::embedded_content_pack;
use etv_core::engine::{EventChoiceInfo, StepEngine, StepMessages};
use etv_core::measure::{build_turn_context, parse_turn_set};
use etv_core::model::{GMProfile, GamePhase, IntentAction, PlayerIntent};
use etv_core::narrator::{
    build_narration_context_with_phase, build_outcome_prompts_for, build_scene_prompts,
    build_scene_prompts_for, check_generated_line, evaluate_turn_matters,
    format_turn_facts_for_slot, CheckReasonCode, CheckerContext, FormatSchema, NarrationContext,
    NarratorLength, PreTurnObservation, PromptRules, PromptSlot, TurnMattersFlags,
    NARRATOR_NUM_CTX, NARRATOR_NUM_PREDICT_CITED,
};
use etv_core::ruleset::Ruleset;
use etv_core::worldgen::create_new_run_with_content;

fn new_engine(seed: i128, ruleset: Ruleset) -> StepEngine {
    let content = embedded_content_pack();
    let mut engine = StepEngine::with_content(create_new_run_with_content(seed, content), content);
    engine.set_ruleset(ruleset);
    engine
}

/// Step the engine and build the turn's context exactly as the play loop does.
fn turn(engine: &mut StepEngine, intent: &PlayerIntent) -> (NarrationContext, StepMessages) {
    let pre = PreTurnObservation::capture(engine.phase, &engine.state);
    let pre_event_id = engine
        .pending_event()
        .map(|e| e.event_id.clone())
        .unwrap_or_default();
    let msgs = engine.step(intent);
    let event_id = engine
        .pending_event()
        .map(|e| e.event_id.clone())
        .unwrap_or(pre_event_id);
    let flags = evaluate_turn_matters(&pre, intent, &msgs, &engine.state);
    let ctx = build_narration_context_with_phase(
        &engine.state,
        engine.phase,
        &event_id,
        &intent.choice_id,
        &msgs,
        flags,
    );
    (ctx, msgs)
}

const RAIN_TEXT: &str = "Rain falls steady as judgment, soaking every seam and hope alike.";

fn has_line(facts: &str, line: &str) -> bool {
    facts.lines().any(|l| l == line)
}

/// Seed 42: the first travel step brings up Persistent Rain.
#[test]
fn a_turn_that_brings_up_an_event_tells_both_calls_what_it_is() {
    let mut engine = new_engine(42, Ruleset::Legacy);
    let (ctx, _) = turn(&mut engine, &PlayerIntent::new(IntentAction::Travel));
    assert_eq!(engine.phase, GamePhase::Event);
    assert_eq!(ctx.event_title(), "Persistent Rain");
    assert_eq!(ctx.check_event_seed(), RAIN_TEXT);
    assert_eq!(
        ctx.offered_labels(),
        ["Continue despite the wet", "Wait under canvas"]
    );
    assert_eq!(ctx.check_choice_label(), "");
    assert_eq!(ctx.action(), "travel");

    for slot in [PromptSlot::Scene, PromptSlot::Outcome] {
        let facts = format_turn_facts_for_slot(&ctx, slot);
        for line in [
            "- event_title: Persistent Rain",
            "- event_text: Rain falls steady as judgment, soaking every seam and hope alike.",
            "- choices_offered: Continue despite the wet | Wait under canvas",
            "- choice_taken: none",
            "- action: travel",
            "- phase: event",
        ] {
            assert!(has_line(&facts, line), "{slot:?} lacks {line:?}\n{facts}");
        }
    }
}

/// The same event, one turn later: the party takes choice A.
#[test]
fn a_turn_that_resolves_a_choice_tells_what_was_chosen_and_only_the_outcome_call_what_resulted() {
    let mut engine = new_engine(42, Ruleset::Legacy);
    turn(&mut engine, &PlayerIntent::new(IntentAction::Travel));
    let (ctx, msgs) = turn(
        &mut engine,
        &PlayerIntent::new(IntentAction::Choose).with_choice("A"),
    );
    assert_eq!(engine.phase, GamePhase::Camp);
    // The step's messages no longer carry the event; the context still knows it.
    assert!(msgs.event_title.is_empty() && msgs.event_choices.is_empty());
    assert_eq!(ctx.event_title(), "Persistent Rain");
    assert_eq!(ctx.check_event_seed(), RAIN_TEXT);
    assert_eq!(ctx.check_choice_label(), "Continue despite the wet");
    assert!(ctx.offered_labels().is_empty());
    assert_eq!(ctx.action(), "");

    let scene = format_turn_facts_for_slot(&ctx, PromptSlot::Scene);
    let outcome = format_turn_facts_for_slot(&ctx, PromptSlot::Outcome);
    for facts in [&scene, &outcome] {
        for line in [
            "- event_title: Persistent Rain",
            "- event_text: Rain falls steady as judgment, soaking every seam and hope alike.",
            "- choices_offered: none",
            "- choice_id: A",
            "- choice_taken: Continue despite the wet",
            "- action: none",
        ] {
            assert!(has_line(facts, line), "lacks {line:?}\n{facts}");
        }
    }
    // The outcome call is the scene call plus what resulted.
    assert!(!scene.contains("WHAT RESULTED"));
    assert!(!scene.contains("resource_directions"));
    assert!(!scene.contains("health_effects"));
    assert_eq!(
        outcome,
        format!(
            "{scene}\nWHAT RESULTED:\n\
- resource_directions: distance:gained, morale:spent, time_days:gained, wagon_condition:spent\n\
- health_effects: none\n\
- turn_matters: death=false, injury=false, supply_zero=false, landmark_arrival=false, night_at_camp=false"
        )
    );

    // And the built prompts carry exactly those two blocks.
    let (scene_sys, _) = build_scene_prompts(GMProfile::Fireside, NarratorLength::Standard, &ctx);
    let (outcome_sys, _) = build_outcome_prompts_for(
        GMProfile::Fireside,
        NarratorLength::Standard,
        &ctx,
        PromptRules::Shipped,
        FormatSchema::Plain,
    );
    assert!(scene_sys.ends_with(&scene));
    assert!(outcome_sys.ends_with(&outcome));
    assert_ne!(scene_sys, outcome_sys);
}

#[test]
fn a_camp_turn_says_what_the_party_did() {
    let mut engine = new_engine(42, Ruleset::Legacy);
    let (ctx, _) = turn(&mut engine, &PlayerIntent::new(IntentAction::Hunt));
    assert_eq!(ctx.action(), "hunt");
    assert_eq!(ctx.event_title(), "");
    let facts = format_turn_facts_for_slot(&ctx, PromptSlot::Scene);
    for line in [
        "- event_title: none",
        "- event_text: none",
        "- choices_offered: none",
        "- choice_taken: none",
        "- action: hunt",
    ] {
        assert!(has_line(&facts, line), "lacks {line:?}\n{facts}");
    }
    let (ctx, _) = turn(&mut engine, &PlayerIntent::new(IntentAction::Rest));
    assert_eq!(ctx.action(), "rest");
}

/// Fallback lines still never reach the model, nor do party names or the engine's own lines.
#[test]
fn the_fallback_line_party_names_and_engine_lines_stay_out_of_every_prompt() {
    let mut engine = new_engine(42, Ruleset::Legacy);
    turn(&mut engine, &PlayerIntent::new(IntentAction::Travel));
    let (ctx, _) = turn(
        &mut engine,
        &PlayerIntent::new(IntentAction::Choose).with_choice("A"),
    );
    assert!(!ctx.fallback_line().is_empty());
    let names: Vec<String> = engine
        .state
        .party
        .members
        .iter()
        .map(|m| m.name.clone())
        .collect();
    assert_eq!(names.len(), 4);
    for profile in [
        GMProfile::Chronicler,
        GMProfile::Fireside,
        GMProfile::Lantern,
    ] {
        for schema in [
            FormatSchema::Plain,
            FormatSchema::CitedFacts,
            FormatSchema::PlainText,
        ] {
            let rules = PromptRules::Shipped;
            let length = NarratorLength::Rich;
            for (sys, user) in [
                build_scene_prompts_for(profile, length, &ctx, rules, schema),
                build_outcome_prompts_for(profile, length, &ctx, rules, schema),
            ] {
                let all = format!("{sys}\n{user}");
                assert!(!all.contains(ctx.fallback_line()));
                for name in &names {
                    assert!(!all.contains(name.as_str()), "{name}");
                }
                for line in ctx.shown_lines() {
                    assert!(!all.contains(line.as_str()), "{line}");
                }
            }
        }
    }
}

/// The checker has the label on the turn it matters. Before this change the play path lost it
/// on the very turn a choice resolved, so the parroting rule never fired there.
#[test]
fn a_line_copying_the_label_is_rejected_and_one_in_its_own_words_passes() {
    let mut engine = new_engine(42, Ruleset::Legacy);
    turn(&mut engine, &PlayerIntent::new(IntentAction::Travel));
    let (ctx, _) = turn(
        &mut engine,
        &PlayerIntent::new(IntentAction::Choose).with_choice("A"),
    );
    let checker = CheckerContext::from_narration_context(&ctx, NarratorLength::Brief);
    assert_eq!(checker.choice_label, "Continue despite the wet");
    assert_eq!(checker.event_seed, RAIN_TEXT);

    // The label, verbatim, as a sentence.
    let failure = check_generated_line("Continue despite the wet.", &checker).unwrap_err();
    assert_eq!(failure.code, CheckReasonCode::ChoiceParroting);
    // The label's verb and noun lifted into a sentence.
    let failure =
        check_generated_line("The party continues on through the wet.", &checker).unwrap_err();
    assert_eq!(failure.code, CheckReasonCode::ChoiceParroting);

    // The same event told in other words passes.
    assert_eq!(
        check_generated_line(
            "The wheels drag through mud as the rain keeps falling.",
            &checker
        ),
        Ok(())
    );
}

/// This is a narrator change, not a rules change: the facts are built the same way under both
/// rulesets.
#[test]
fn legacy_and_v2_build_the_same_facts_for_the_same_turn() {
    let mut legacy = new_engine(42, Ruleset::Legacy);
    let mut v2 = new_engine(42, Ruleset::V2);
    let (a, _) = turn(&mut legacy, &PlayerIntent::new(IntentAction::Travel));
    let (b, _) = turn(&mut v2, &PlayerIntent::new(IntentAction::Travel));
    for slot in [PromptSlot::Scene, PromptSlot::Outcome] {
        assert_eq!(
            format_turn_facts_for_slot(&a, slot),
            format_turn_facts_for_slot(&b, slot)
        );
    }
    let (a, _) = turn(
        &mut legacy,
        &PlayerIntent::new(IntentAction::Choose).with_choice("A"),
    );
    let (b, _) = turn(
        &mut v2,
        &PlayerIntent::new(IntentAction::Choose).with_choice("A"),
    );
    for slot in [PromptSlot::Scene, PromptSlot::Outcome] {
        assert_eq!(
            format_turn_facts_for_slot(&a, slot),
            format_turn_facts_for_slot(&b, slot)
        );
    }
}

/// A version 1 turn rebuilt by the harness now carries its event text and label too.
#[test]
fn harness_v1_turns_carry_the_event_text_and_the_label() {
    let set = parse_turn_set(
        include_bytes!("fixtures/narrator_turn_set_v1.json"),
        "838e4346263507a112b6a161a4107a3647c253a39646b2427236afdd4e526f17",
    )
    .unwrap();
    let t = &set.turns[0];
    assert_eq!((t.id.as_str(), t.event_id.as_str()), ("w0-001", "f5_092"));
    let ctx = build_turn_context(t, GMProfile::Chronicler);
    let facts = format_turn_facts_for_slot(&ctx, PromptSlot::Scene);
    assert!(has_line(
        &facts,
        "- event_text: The water shines like spilled mercury under the moon."
    ));
    assert!(has_line(&facts, "- choice_taken: Wait for dawn"));
    // Version 1 stores no title.
    assert!(has_line(&facts, "- event_title: none"));
}

/// The request fits the context window with room for the answer, on the longest event in the
/// content and the longest prompt variant.
///
/// There is no tokenizer here, so the budget is counted in characters at three characters to a
/// token, which over-counts ordinary English (about four). The answer's own cap is added.
#[test]
fn the_longest_request_fits_the_context_window() {
    let content = embedded_content_pack();
    assert_eq!(content.all_events.len(), 260);
    let state = create_new_run_with_content(42, content);
    let mut longest_chars = 0usize;
    let mut longest_event = String::new();
    for event in &content.all_events {
        let choices: Vec<EventChoiceInfo> = event
            .fallback_choices
            .iter()
            .map(|c| EventChoiceInfo {
                id: c.choice_id.clone(),
                label: c.label.clone(),
                risk_hint: c.risk_hint.clone(),
                cost_hint: c.cost_hint.clone(),
                ..Default::default()
            })
            .collect();
        let msgs = StepMessages {
            event_title: event.title.clone(),
            event_narration: event.fallback_narration.clone(),
            event_choices: choices,
            ..StepMessages::default()
        };
        // An event on screen with every choice listed is the longest facts block.
        let ctx = build_narration_context_with_phase(
            &state,
            GamePhase::Event,
            &event.event_id,
            "",
            &msgs,
            TurnMattersFlags::default(),
        );
        for profile in [
            GMProfile::Chronicler,
            GMProfile::Fireside,
            GMProfile::Lantern,
        ] {
            for rules in [PromptRules::Shipped, PromptRules::Positive] {
                let (sys, user) = build_outcome_prompts_for(
                    profile,
                    NarratorLength::Rich,
                    &ctx,
                    rules,
                    FormatSchema::CitedFacts,
                );
                let chars = sys.chars().count() + user.chars().count();
                if chars > longest_chars {
                    longest_chars = chars;
                    longest_event = event.event_id.clone();
                }
            }
        }
    }
    let estimated_prompt_tokens = longest_chars.div_ceil(3);
    let with_answer = estimated_prompt_tokens + NARRATOR_NUM_PREDICT_CITED as usize;
    assert!(
        with_answer <= NARRATOR_NUM_CTX as usize,
        "longest request ({longest_event}): {longest_chars} chars, about {estimated_prompt_tokens} tokens, {with_answer} with the answer, window {NARRATOR_NUM_CTX}"
    );
    // Measured when this was written; a change here means a prompt or content change.
    assert_eq!(
        (longest_event.as_str(), longest_chars),
        (EXPECTED_LONGEST_EVENT, EXPECTED_LONGEST_CHARS)
    );
}

const EXPECTED_LONGEST_EVENT: &str = "fog_bell";
const EXPECTED_LONGEST_CHARS: usize = 2608;
