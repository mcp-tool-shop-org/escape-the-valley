//! Three narrator rulings, checked on the real turns of turn set v2:
//!
//! 1. A death may be stated only where it happened: on a turn where a member died, or where the
//!    run ended lost or pyrrhic. The rule text in the prompt and the checker change together.
//! 2. `resource_directions` uses `spent`/`gained` for supplies only. Health and morale fall and
//!    rise, days pass, and distance is told as what it was on that turn.
//! 3. The caravan's doctrine and taboo reach the narrator labelled as such, and no doctrine or
//!    taboo line says anything about the party's morale.
//!
//! No model is called anywhere in this file.

use etv_core::measure::{parse_turn_set_v2, TurnSpecV2};
use etv_core::narrator::{
    build_outcome_prompts, build_outcome_prompts_with_rules, build_scene_prompts, change_word,
    check_generated_line, death_may_be_stated, format_turn_facts_for_slot, situation_for_narrator,
    CheckReasonCode, CheckerContext, NarratorLength, PromptRules, PromptSlot, DEATH_RULE_ALLOWED,
    DEATH_RULE_BANNED, SITUATION_RELABELS,
};

const V2_SHA256: &str = "518beeccde989abc20b08ada42b2b3d582755bd5c4bd0f39b05f31204323535e";
const V2_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v2.json");

fn turns() -> Vec<TurnSpecV2> {
    parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap().turns
}

fn turn(id: &str) -> TurnSpecV2 {
    turns().into_iter().find(|t| t.id == id).unwrap()
}

fn checker(t: &TurnSpecV2) -> CheckerContext {
    CheckerContext::from_narration_context(&t.context, NarratorLength::Brief)
}

/// A plain statement of a death: no name, no sentiment, no gore.
const DEATH_LINE: &str = "A traveller died before the camp was struck.";

// ---- 1. death, allowed only where it happened --------------------------------------------------

#[test]
fn the_set_has_35_turns_where_a_death_may_be_stated_and_325_where_it_may_not() {
    let turns = turns();
    let mut died_no_ending = 0;
    let mut lost = 0;
    let mut pyrrhic = 0;
    let mut triumphant = 0;
    let mut allowed = 0;
    for t in &turns {
        let c = &t.context;
        match c.ending() {
            "lost" => lost += 1,
            "pyrrhic" => pyrrhic += 1,
            "triumphant" => triumphant += 1,
            "" => {
                if c.flags().death {
                    died_no_ending += 1;
                }
            }
            other => panic!("unexpected ending {other:?}"),
        }
        let expected = c.flags().death || c.ending() == "lost" || c.ending() == "pyrrhic";
        assert_eq!(death_may_be_stated(c), expected, "{}", t.id);
        assert_eq!(checker(t).death_allowed, expected, "{}", t.id);
        if expected {
            allowed += 1;
        }
        // A triumphant ending never allows it: nobody died on those turns.
        if c.ending() == "triumphant" {
            assert!(!c.flags().death);
            assert!(!death_may_be_stated(c));
        }
    }
    assert_eq!(turns.len(), 360);
    assert_eq!((died_no_ending, lost, pyrrhic, triumphant), (25, 8, 2, 4));
    assert_eq!(allowed, 35);
}

#[test]
fn the_prompt_carries_the_allowing_rule_on_exactly_those_turns_in_both_calls() {
    for t in turns() {
        let c = &t.context;
        let allowed = death_may_be_stated(c);
        let (scene_system, _) = build_scene_prompts(c.gm_profile(), NarratorLength::Brief, c);
        let (outcome_system, _) = build_outcome_prompts(c.gm_profile(), NarratorLength::Brief, c);
        for system in [&scene_system, &outcome_system] {
            assert_eq!(system.contains(DEATH_RULE_ALLOWED), allowed, "{}", t.id);
            assert_eq!(system.contains(DEATH_RULE_BANNED), !allowed, "{}", t.id);
        }
        // The positively worded rules follow the same turns.
        let (positive, _) = build_outcome_prompts_with_rules(
            c.gm_profile(),
            NarratorLength::Brief,
            c,
            PromptRules::Positive,
        );
        assert_eq!(
            positive.contains("say so plainly and briefly"),
            allowed,
            "{}",
            t.id
        );
        assert_eq!(
            positive.contains("leave every traveller as the facts give them"),
            !allowed,
            "{}",
            t.id
        );
    }
}

#[test]
fn the_rule_text_is_the_ruled_wording() {
    assert_eq!(
        DEATH_RULE_BANNED,
        "- Never state or imply death or total ruin."
    );
    assert_eq!(
        DEATH_RULE_ALLOWED,
        "- A death or the journey's failure reported in the turn facts may be stated plainly: \
         without sentiment, without a name, without gore. Never state or imply any other death \
         or ruin."
    );
}

/// v2-w0-001: Thomas starved on this turn; the run goes on.
#[test]
fn a_death_line_passes_on_a_death_turn() {
    let t = turn("v2-w0-001");
    assert!(t.context.flags().death);
    assert_eq!(t.context.ending(), "");
    assert_eq!(check_generated_line(DEATH_LINE, &checker(&t)), Ok(()));
}

#[test]
fn a_death_line_passes_on_a_lost_and_on_a_pyrrhic_ending() {
    let turns = turns();
    for ending in ["lost", "pyrrhic"] {
        let found: Vec<&TurnSpecV2> = turns
            .iter()
            .filter(|t| t.context.ending() == ending)
            .collect();
        assert!(!found.is_empty());
        for t in found {
            assert_eq!(
                check_generated_line(DEATH_LINE, &checker(t)),
                Ok(()),
                "{} ({ending})",
                t.id
            );
        }
    }
    // The two pyrrhic endings carry no death on the turn itself: the ending alone allows it.
    assert!(turns
        .iter()
        .filter(|t| t.context.ending() == "pyrrhic")
        .all(|t| !t.context.flags().death));
}

/// v2-w0-006: an ordinary travel turn. v2-w0-023: the arrival that wins the run, all four alive.
#[test]
fn the_same_line_is_rejected_on_an_ordinary_turn_and_on_a_triumphant_ending() {
    let ordinary = turn("v2-w0-006");
    assert!(!ordinary.context.flags().death);
    assert_eq!(ordinary.context.ending(), "");
    let triumphant = turn("v2-w0-023");
    assert_eq!(triumphant.context.ending(), "triumphant");
    for t in [&ordinary, &triumphant] {
        let err = check_generated_line(DEATH_LINE, &checker(t)).unwrap_err();
        assert_eq!(err.code, CheckReasonCode::DeathWords, "{}", t.id);
        assert_eq!(err.detail, "banned death word 'died'");
    }
}

#[test]
fn every_turn_where_death_is_not_allowed_rejects_every_plain_death_word() {
    let lines = [
        "Someone here is dead.",
        "A traveller died.",
        "Someone dies tonight.",
        "The weakest is dying.",
        "Death walks behind the wagon.",
        "There will be deaths before the pass.",
        "A grave waits by the road.",
        "Graves wait by the road.",
        "They buried a friend.",
        "A burial slows the morning.",
        "Some will perish here.",
        "A friend perished.",
        "Hope perishes on this road.",
    ];
    for t in turns() {
        if death_may_be_stated(&t.context) {
            continue;
        }
        let ctx = checker(&t);
        for line in lines {
            let err = check_generated_line(line, &ctx).unwrap_err();
            assert_eq!(err.code, CheckReasonCode::DeathWords, "{} {line:?}", t.id);
        }
    }
}

#[test]
fn on_a_death_turn_gore_violence_ruin_and_names_are_still_rejected() {
    let t = turn("v2-w0-001");
    let ctx = checker(&t);
    assert!(ctx.death_allowed);

    let gore = check_generated_line("A traveller died in the blood and mud.", &ctx).unwrap_err();
    assert_eq!(gore.code, CheckReasonCode::DeathWords);
    assert_eq!(gore.detail, "gore word 'blood' on a death turn");

    for (line, word) in [
        ("The road killed a traveller.", "killed"),
        ("A corpse lay by the wagon.", "corpse"),
        ("A traveller lay lifeless at dawn.", "lifeless"),
        ("The whole company is destroyed.", "destroyed"),
        ("The sickness proved fatal.", "fatal"),
    ] {
        let err = check_generated_line(line, &ctx).unwrap_err();
        assert_eq!(err.code, CheckReasonCode::DeathWords, "{line:?}");
        assert_eq!(err.detail, format!("banned death word '{word}'"));
    }

    // The member who died is still never named.
    assert_eq!(t.context.check_party_names()[0], "Thomas");
    let named = check_generated_line("Thomas died before the camp was struck.", &ctx).unwrap_err();
    assert_ne!(named.code, CheckReasonCode::DeathWords);
}

/// Gore words are a death-turn rule only: the ordinary checker is exactly as it was.
#[test]
fn the_gore_list_does_not_touch_ordinary_turns() {
    let t = turn("v2-w0-006");
    let ctx = checker(&t);
    assert!(!ctx.death_allowed);
    assert_eq!(
        check_generated_line("A scraped knee left blood on the wheel.", &ctx),
        Ok(())
    );
    // A context built by hand (as older callers do) defaults to the ban.
    assert!(!CheckerContext::default().death_allowed);
}

// ---- 2. resource_directions wording ------------------------------------------------------------

#[test]
fn change_words_by_key() {
    // Supplies keep spent and gained.
    for key in [
        "food", "water", "ammo", "parts", "firewood", "oil", "rope", "tools", "cloth",
    ] {
        assert_eq!(change_word(key, -1, false), "spent");
        assert_eq!(change_word(key, 2, false), "gained");
        assert_eq!(change_word(key, -1, true), "spent");
        assert_eq!(change_word(key, 2, true), "gained");
    }
    // Conditions fall and rise.
    for key in ["health", "morale", "wagon_condition", "animals_health"] {
        assert_eq!(change_word(key, -3, false), "fell");
        assert_eq!(change_word(key, 3, false), "rose");
        assert_eq!(change_word(key, -3, true), "fell");
        assert_eq!(change_word(key, 3, true), "rose");
    }
    // Days pass.
    assert_eq!(change_word("time_days", 1, false), "days passed");
    assert_eq!(change_word("time_days", 1, true), "days passed");
    assert_eq!(change_word("time_days", -1, true), "time saved");
    // Distance at camp is miles covered. On a resolved choice the engine reports miles still to
    // go, so a positive change is a longer road.
    assert_eq!(change_word("distance", 5, false), "travelled");
    assert_eq!(change_word("distance", -5, false), "lost ground");
    assert_eq!(change_word("distance", 5, true), "road longer");
    assert_eq!(change_word("distance", -5, true), "road shorter");
}

fn directions_line(t: &TurnSpecV2) -> String {
    format_turn_facts_for_slot(&t.context, PromptSlot::Outcome)
        .lines()
        .find(|l| l.starts_with("- resource_directions: "))
        .unwrap()
        .to_string()
}

/// v2-w0-006: travelled 5 miles; food -1, morale +1, a day, water -1.
#[test]
fn a_camp_travel_turn_reads_travelled_days_passed_and_rose() {
    let t = turn("v2-w0-006");
    assert_eq!(t.context.choice_id(), "");
    assert_eq!(
        directions_line(&t),
        "- resource_directions: distance:travelled, food:spent, morale:rose, \
         time_days:days passed, water:spent"
    );
}

/// v2-w0-001: travelled 3 miles; health -36, morale -11, a day, water -2.
#[test]
fn health_and_morale_fall_rather_than_being_spent() {
    let t = turn("v2-w0-001");
    assert_eq!(
        directions_line(&t),
        "- resource_directions: distance:travelled, health:fell, morale:fell, \
         time_days:days passed, water:spent"
    );
}

#[test]
fn no_turn_in_the_set_spends_or_gains_anything_but_a_supply() {
    const NOT_SUPPLIES: [&str; 6] = [
        "health",
        "morale",
        "wagon_condition",
        "animals_health",
        "time_days",
        "distance",
    ];
    let mut seen_road = 0;
    for t in turns() {
        let line = directions_line(&t);
        let body = line.trim_start_matches("- resource_directions: ");
        if body == "none" {
            continue;
        }
        for item in body.split(", ") {
            let (key, word) = item.split_once(':').unwrap();
            if word == "spent" || word == "gained" {
                assert!(!NOT_SUPPLIES.contains(&key), "{} {item}", t.id);
            } else {
                assert!(NOT_SUPPLIES.contains(&key), "{} {item}", t.id);
            }
            if key == "distance" {
                let on_choice = !t.context.choice_id().is_empty();
                assert_eq!(word.starts_with("road "), on_choice, "{} {item}", t.id);
                if on_choice {
                    seen_road += 1;
                }
            }
        }
    }
    assert!(seen_road > 0);
}

// ---- 3. doctrine and taboo lines ---------------------------------------------------------------

#[test]
fn each_engine_doctrine_and_taboo_sentence_is_relabelled() {
    let cases = [
        (
            "The caravan travels light — less to eat, less to carry.",
            "Their doctrine: travel light.",
        ),
        (
            "Every repair is deliberate. The caravan moves carefully.",
            "Their doctrine: careful hands.",
        ),
        (
            "They carry no debts. Morale holds, but the wagon is lean.",
            "Their doctrine: no debts.",
        ),
        (
            "They will not travel at night.",
            "Their taboo: no travel at night.",
        ),
        (
            "They refuse to drink river water.",
            "Their taboo: no river water.",
        ),
        (
            "They leave nothing behind.",
            "Their taboo: leave nothing behind.",
        ),
    ];
    assert_eq!(SITUATION_RELABELS, cases);
    for (from, to) in cases {
        assert_eq!(situation_for_narrator(from), to);
    }
    assert_eq!(
        situation_for_narrator(
            "Day 28, night. Swamp terrain. Well past halfway. 1 of 4 party members alive. \
             They have no food. They carry no debts. Morale holds, but the wagon is lean. \
             They refuse to drink river water."
        ),
        "Day 28, night. Swamp terrain. Well past halfway. 1 of 4 party members alive. \
         They have no food. Their doctrine: no debts. Their taboo: no river water."
    );
}

/// v2-w0-003 is the turn that showed the defect: one of four alive, morale low, and the old line
/// said morale holds.
#[test]
fn the_turn_that_showed_the_defect_no_longer_says_morale_holds() {
    let t = turn("v2-w0-003");
    assert_eq!(t.context.party_status().alive_count(), 1);
    assert_eq!(t.context.party_status().morale_band(), "low");
    assert_eq!(
        t.context.situation(),
        "Day 28, night. Swamp terrain. Well past halfway. 1 of 4 party members alive. \
         They have no food. Their doctrine: no debts. Their taboo: no river water."
    );
}

#[test]
fn no_doctrine_or_taboo_line_in_the_set_asserts_a_morale_state() {
    let mut doctrine = [0usize; 3];
    let mut taboo = [0usize; 3];
    for t in turns() {
        let situation = t.context.situation();
        let sentences: Vec<&str> = situation
            .split(". ")
            .map(|s| s.trim_end_matches('.'))
            .collect();
        let mut doctrines_here = 0;
        let mut taboos_here = 0;
        for s in &sentences {
            let lower = s.to_lowercase();
            if let Some(rest) = s.strip_prefix("Their doctrine: ") {
                doctrines_here += 1;
                let i = ["travel light", "careful hands", "no debts"]
                    .iter()
                    .position(|d| *d == rest)
                    .unwrap_or_else(|| panic!("{} unknown doctrine {rest:?}", t.id));
                doctrine[i] += 1;
            } else if let Some(rest) = s.strip_prefix("Their taboo: ") {
                taboos_here += 1;
                let i = [
                    "no travel at night",
                    "no river water",
                    "leave nothing behind",
                ]
                .iter()
                .position(|d| *d == rest)
                .unwrap_or_else(|| panic!("{} unknown taboo {rest:?}", t.id));
                taboo[i] += 1;
            } else if lower.contains("spirit") {
                // The one line about the party's spirit comes from the engine's own pressure
                // reading, and only when morale really is broken.
                assert_eq!(*s, "The party's spirit is shattered", "{}", t.id);
                assert_eq!(t.context.pressures()[0], "MORALE_BROKEN", "{}", t.id);
            }
            // No sentence of any kind claims morale holds.
            assert!(!lower.contains("morale"), "{} {s:?}", t.id);
            assert!(!lower.contains("holds"), "{} {s:?}", t.id);
        }
        assert_eq!((doctrines_here, taboos_here), (1, 1), "{}", t.id);
        // None of the engine's unlabelled sentences survive.
        for (from, _) in SITUATION_RELABELS {
            assert!(!situation.contains(from), "{}", t.id);
        }
    }
    assert_eq!(doctrine, [161, 65, 134]);
    assert_eq!(taboo, [107, 145, 108]);
}

/// The allowing rule is longer than the ban. Every turn of the set, in the longest arm, still
/// fits the context window with the answer (three characters to a token, as elsewhere).
#[test]
fn every_turn_still_fits_the_context_window_with_the_longer_rule() {
    use etv_core::narrator::{
        build_outcome_prompts_for, FormatSchema, NARRATOR_NUM_CTX, NARRATOR_NUM_PREDICT_CITED,
    };
    let mut longest = 0;
    for t in turns() {
        let c = &t.context;
        let (system, user) = build_outcome_prompts_for(
            c.gm_profile(),
            NarratorLength::Rich,
            c,
            PromptRules::Shipped,
            FormatSchema::CitedFacts,
        );
        longest = longest.max(system.chars().count() + user.chars().count());
    }
    let with_answer = longest.div_ceil(3) + NARRATOR_NUM_PREDICT_CITED as usize;
    assert!(
        with_answer <= NARRATOR_NUM_CTX as usize,
        "{longest} chars, {with_answer} tokens with the answer"
    );
}
