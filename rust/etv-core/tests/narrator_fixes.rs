//! The narrator fixes, each behind its own experiment arm and off in the game: the two checker
//! changes (choice parroting verbatim only; the repeated opening on three words with the lead's
//! subjects exempt), the pyrrhic loss word, the choice's result in the lead, the choice-conveyed
//! rule, the scene slot's repeated-opening rule, the anachronism list, the lead guard with its
//! time-of-day rule, and the present-tense lead.
//!
//! The turn facts are real turns of turn set v2. The lines are written for the tests. No model is
//! called anywhere in this file.

use etv_core::measure::{parse_turn_set_v2, TurnSpecV2};
use etv_core::model::GMProfile;
use etv_core::narrator::{
    build_outcome_prompts, check_generated_line, verbatim_label_copy, CheckReasonCode,
    CheckerContext, NarratorLength,
};
use etv_core::narrator_experiment::{
    checker_context_for, lead_outcome_system_prompt, lead_outcome_system_prompt_for,
    ExperimentArms, OUTCOME_LEAD_HEADING_GUARD,
};
use etv_core::narrator_truth::{
    opening_words, stem, what_happened, what_happened_in, words_beyond_lead, LeadStyle,
    LEAD_SUBJECTS,
};

const V2_SHA256: &str = "59a3e4f8870474d01b775a52071fae9a12629cec0f225606271e9d77f5f8ca46";
const V2_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v2.json");

fn turn(id: &str) -> TurnSpecV2 {
    parse_turn_set_v2(V2_BYTES, V2_SHA256)
        .unwrap()
        .turns
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

/// The first rule `line` breaks on turn `id` under `arms`, or `None` when it passes.
fn first_break(
    id: &str,
    length: NarratorLength,
    arms: ExperimentArms,
    is_outcome: bool,
    previous: Option<&str>,
    line: &str,
) -> Option<CheckReasonCode> {
    let ctx = turn(id).context;
    let c = checker_context_for(
        &ctx,
        length,
        GMProfile::Chronicler,
        arms,
        is_outcome,
        previous,
    );
    check_generated_line(line, &c).err().map(|f| f.code)
}

fn arms(f: impl FnOnce(&mut ExperimentArms)) -> ExperimentArms {
    let mut a = ExperimentArms::default();
    f(&mut a);
    a
}

// ---- choice parroting, verbatim only ---------------------------------------------------------------

#[test]
fn verbatim_label_copy_needs_the_whole_label_or_five_words_in_a_row() {
    let t = |line: &str| -> Vec<String> {
        line.split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                    .to_lowercase()
            })
            .collect()
    };
    let label = "Give what you can spare";
    assert_eq!(
        verbatim_label_copy(&t("They give what you can spare and go on."), label),
        Some("give what you can spare".to_string())
    );
    // Four of the five words in a row is not a copy of a five-word label.
    assert_eq!(
        verbatim_label_copy(&t("They give what you can."), label),
        None
    );
    // Naming what the party did passes.
    assert_eq!(
        verbatim_label_copy(&t("The party gives food to the family."), label),
        None
    );
    // A longer label: five of its words in a row are a copy.
    let long = "Pay the toll and cross the bridge at once";
    assert_eq!(
        verbatim_label_copy(&t("They cross the bridge at once."), long),
        Some("cross the bridge at once".to_string())
    );
    assert_eq!(verbatim_label_copy(&t("They pay the toll."), long), None);
    // A two-word label must appear whole; a one-word label is never a parrot.
    assert_eq!(
        verbatim_label_copy(&t("The party will rest here."), "Rest here"),
        Some("rest here".to_string())
    );
    assert_eq!(verbatim_label_copy(&t("The party rests."), "Rest"), None);
}

#[test]
fn the_parrot_verbatim_arm_replaces_the_verb_and_noun_rule() {
    let ctx = CheckerContext {
        choice_label: "Repair the axle".to_string(),
        ..CheckerContext::default()
    };
    // The shipped rule rejects the label's verb and noun anywhere in the line.
    let line = "The axle holds once the party repairs it.";
    assert_eq!(
        check_generated_line(line, &ctx).err().map(|f| f.code),
        Some(CheckReasonCode::ChoiceParroting)
    );
    let arm = CheckerContext {
        parrot_verbatim: true,
        ..ctx.clone()
    };
    assert_eq!(check_generated_line(line, &arm), Ok(()));
    assert_eq!(
        check_generated_line("They repair the axle by the river.", &arm)
            .err()
            .map(|f| f.code),
        Some(CheckReasonCode::ChoiceParroting)
    );
}

// ---- the repeated opening, three words --------------------------------------------------------------

#[test]
fn the_three_word_arm_compares_three_words_and_exempts_the_leads_subjects() {
    assert_eq!(opening_words("Mud grips the wheels.", 3), "mud grips the");
    let two = arms(|a| a.no_repeat_opening = true);
    let three = arms(|a| {
        a.no_repeat_opening = true;
        a.opening_three_words = true;
    });
    let check = |a, prev, line| {
        first_break(
            "v2-w0-006",
            NarratorLength::Standard,
            a,
            true,
            Some(prev),
            line,
        )
    };
    // Two words alike: a repeat under the old rule, not under three words.
    assert_eq!(
        check(two, "mud grips the", "Mud grips a tired team."),
        Some(CheckReasonCode::RepeatedOpening)
    );
    assert_eq!(
        check(three, "mud grips the", "Mud grips a tired team."),
        None
    );
    assert_eq!(
        check(three, "mud grips the", "Mud grips the wheels again."),
        Some(CheckReasonCode::RepeatedOpening)
    );
    // The lead's own subjects are exempt, whatever follows.
    assert_eq!(
        LEAD_SUBJECTS,
        [
            "the party",
            "a traveller",
            "the last",
            "the turn",
            "the wagon"
        ]
    );
    assert_eq!(
        check(
            two,
            "the party travels",
            "The party travels on through the flats."
        ),
        Some(CheckReasonCode::RepeatedOpening)
    );
    assert_eq!(
        check(
            three,
            "the party travels",
            "The party travels on through the flats."
        ),
        None
    );
}

// ---- the scene slot ---------------------------------------------------------------------------------

#[test]
fn the_scene_rules_arm_holds_the_scene_line_to_the_repeated_opening_rule() {
    let off = arms(|a| a.no_repeat_opening = true);
    let on = arms(|a| {
        a.no_repeat_opening = true;
        a.scene_rules = true;
    });
    let line = "Mud swells under the wheels.";
    assert_eq!(
        first_break(
            "v2-w0-006",
            NarratorLength::Brief,
            off,
            false,
            Some("mud swells under"),
            line
        ),
        None
    );
    assert_eq!(
        first_break(
            "v2-w0-006",
            NarratorLength::Brief,
            on,
            false,
            Some("mud swells under"),
            line
        ),
        Some(CheckReasonCode::RepeatedOpening)
    );
}

#[test]
fn the_anachronism_arm_rejects_tires_engines_and_phones_in_either_slot() {
    let on = arms(|a| a.no_anachronism = true);
    for is_outcome in [false, true] {
        assert_eq!(
            first_break(
                "v2-w0-006",
                NarratorLength::Brief,
                on,
                is_outcome,
                None,
                "Mud swells under tires."
            ),
            Some(CheckReasonCode::Anachronism)
        );
        assert_eq!(
            first_break(
                "v2-w0-006",
                NarratorLength::Brief,
                on,
                is_outcome,
                None,
                "Mud swells under the wheels."
            ),
            None
        );
    }
    assert_eq!(
        first_break(
            "v2-w0-006",
            NarratorLength::Brief,
            ExperimentArms::default(),
            false,
            None,
            "Mud swells under tires."
        ),
        None
    );
}

// ---- the pyrrhic ending -------------------------------------------------------------------------------

#[test]
fn the_pyrrhic_arm_wants_a_word_of_what_the_journey_cost() {
    // v2-w0-025 ends pyrrhic at Copperhill.
    let swh = arms(|a| a.says_what_happened = true);
    let on = arms(|a| {
        a.says_what_happened = true;
        a.pyrrhic_loss_word = true;
    });
    let bare = "The party reached Copperhill.";
    assert_eq!(
        first_break("v2-w0-025", NarratorLength::Brief, swh, true, None, bare),
        None
    );
    assert_eq!(
        first_break("v2-w0-025", NarratorLength::Brief, on, true, None, bare),
        Some(CheckReasonCode::PyrrhicLossNotConveyed)
    );
    assert_eq!(
        first_break(
            "v2-w0-025",
            NarratorLength::Brief,
            on,
            true,
            None,
            "The party reached Copperhill with fewer than set out."
        ),
        None
    );
    // The scene line is not asked for it.
    assert_eq!(
        first_break("v2-w0-025", NarratorLength::Brief, on, false, None, bare),
        None
    );
}

// ---- the lead ---------------------------------------------------------------------------------------

#[test]
fn the_lead_in_the_present_and_from_the_choices_result() {
    let present = LeadStyle {
        present: true,
        choice_result: false,
    };
    let result = LeadStyle {
        present: false,
        choice_result: true,
    };
    let both = LeadStyle {
        present: true,
        choice_result: true,
    };
    let lead = |id: &str, style| what_happened_in(&turn(id).context, style);
    // The default style is the lead as first measured.
    for id in ["v2-w0-001", "v2-w0-014", "v2-w0-025", "v2-w0-006"] {
        assert_eq!(
            lead(id, LeadStyle::default()),
            what_happened(&turn(id).context)
        );
    }
    assert_eq!(lead("v2-w0-001", present), "A traveller dies.");
    assert_eq!(
        lead("v2-w0-025", present),
        "The party reaches Copperhill and the end of the trail, with some of its number lost on the way. The journey is over."
    );
    assert_eq!(lead("v2-w0-006", present), "The party travels on.");
    // v2-w0-014: the party chose "Give what you can spare"; food fell by four, morale rose.
    assert_eq!(
        lead("v2-w0-014", present),
        "The party chooses: Give what you can spare."
    );
    assert_eq!(lead("v2-w0-014", result), "The choice cost the party food.");
    assert_eq!(lead("v2-w0-014", both), "The choice costs the party food.");
    // The result style changes only choice turns.
    assert_eq!(lead("v2-w0-001", result), "A traveller died.");
}

#[test]
fn the_lead_guard_heading_and_lead_reach_the_outcome_prompt() {
    let ctx = turn("v2-w0-014").context;
    let (system, _) = build_outcome_prompts(GMProfile::Fireside, NarratorLength::Brief, &ctx);
    let a = arms(|a| {
        a.outcome_lead = true;
        a.lead_guard = true;
        a.present_lead = true;
        a.choice_result_lead = true;
    });
    let prompt = lead_outcome_system_prompt_for(&system, &ctx, a);
    let head = format!(
        "{OUTCOME_LEAD_HEADING_GUARD}\n- The choice costs the party food.\n\nTURN FACTS:\n"
    );
    assert_eq!(prompt.matches(&head).count(), 1, "{prompt}");
    // With no arm, the prompt is the one first measured.
    assert_eq!(
        lead_outcome_system_prompt_for(&system, &ctx, ExperimentArms::default()),
        lead_outcome_system_prompt(&system, &ctx)
    );
}

// ---- the choice is conveyed ----------------------------------------------------------------------------

#[test]
fn the_choice_conveyed_arm_wants_the_choices_verb_or_a_supply_it_changed() {
    // v2-w0-014: "Give what you can spare"; food was spent.
    let on = arms(|a| a.choice_conveyed = true);
    let check = |line| first_break("v2-w0-014", NarratorLength::Standard, on, true, None, line);
    assert_eq!(
        check("Night settles over The Long Descent and the family walks away."),
        Some(CheckReasonCode::ChoiceNotConveyed)
    );
    assert_eq!(check("The party gives freely under the night sky."), None);
    assert_eq!(
        check("Food leaves the wagon in small bundles tonight."),
        None
    );
    // A turn with no choice is not asked.
    assert_eq!(
        first_break(
            "v2-w0-006",
            NarratorLength::Standard,
            on,
            true,
            None,
            "Dust crusts the wheels."
        ),
        None
    );
    assert_eq!(stem("gives"), "giv");
    assert_eq!(stem("give"), "giv");
}

// ---- the lead guard ------------------------------------------------------------------------------------

#[test]
fn the_lead_guard_wants_one_word_beyond_the_lead_at_brief_and_two_after() {
    // v2-w0-001: a traveller died, in the afternoon, at The Long Descent (a swamp).
    let guard = arms(|a| {
        a.outcome_lead = true;
        a.lead_guard = true;
    });
    let check = |length, line| first_break("v2-w0-001", length, guard, true, None, line);
    assert_eq!(
        check(NarratorLength::Brief, "A traveller died."),
        Some(CheckReasonCode::LeadOnly)
    );
    // Another tense of the same words adds nothing.
    assert_eq!(
        check(NarratorLength::Brief, "A traveller dies."),
        Some(CheckReasonCode::LeadOnly)
    );
    assert_eq!(
        check(NarratorLength::Brief, "A traveller died in the swamp."),
        None
    );
    assert_eq!(
        check(NarratorLength::Standard, "A traveller died in the swamp."),
        Some(CheckReasonCode::LeadOnly)
    );
    assert_eq!(
        check(
            NarratorLength::Standard,
            "A traveller died in the swamp at midday."
        ),
        None
    );
    assert_eq!(
        words_beyond_lead(
            "A traveller died in the swamp at midday.",
            "A traveller died."
        ),
        ["swamp", "midday"]
    );
    // Without the outcome lead there is no lead to guard.
    let no_lead = arms(|a| a.lead_guard = true);
    assert_eq!(
        first_break(
            "v2-w0-001",
            NarratorLength::Brief,
            no_lead,
            true,
            None,
            "A traveller died."
        ),
        None
    );
}

#[test]
fn the_detail_must_not_invent_a_place_or_contradict_the_time_of_day() {
    // v2-w0-001 is in the afternoon at The Long Descent.
    let guard = arms(|a| {
        a.outcome_lead = true;
        a.lead_guard = true;
    });
    let check = |line| first_break("v2-w0-001", NarratorLength::Brief, guard, true, None, line);
    // A made-up place is a mid-sentence capital, which the shipped checker already rejects.
    assert_eq!(
        check("A traveller died near Gallowmoor."),
        Some(CheckReasonCode::MidSentenceCapital)
    );
    // The turn's own place passes.
    assert_eq!(check("A traveller died on The Long Descent."), None);
    // The afternoon may become evening or morning, never night.
    assert_eq!(check("A traveller died as the evening came."), None);
    assert_eq!(
        check("A traveller died under the midnight sky."),
        Some(CheckReasonCode::TimeOfDayContradiction)
    );
}

#[test]
fn every_new_arm_has_a_flag_and_a_label() {
    let all = arms(|a| {
        a.parrot_verbatim = true;
        a.opening_three_words = true;
        a.pyrrhic_loss_word = true;
        a.choice_result_lead = true;
        a.choice_conveyed = true;
        a.scene_rules = true;
        a.no_anachronism = true;
        a.lead_guard = true;
        a.present_lead = true;
    });
    assert_eq!(
        all.label(),
        "parrot-verbatim+opening-three-words+pyrrhic-loss-word+choice-result-lead+choice-conveyed+scene-rules+no-anachronism+lead-guard+present-lead"
    );
    assert!(ExperimentArms::default().is_shipped());
}

// ---- the taboo, the doctrine and invented water ---------------------------------------------------

/// Session 5's qwen3:8b line #20 of its reading sample (turn v2-w0-018: The Scar, alpine,
/// resting, water spent, taboo "no river water"). It was accepted by the shipped checker.
const KNELT_TO_DRINK: &str = "The party rested by the riverbank where the water ran cold and clear. A few of them knelt to drink but none took more than a mouthful.";

#[test]
fn the_taboo_arm_rejects_the_s5_line_that_drinks_river_water() {
    let off = arms(|a| a.says_what_happened = true);
    let on = arms(|a| a.taboo_rules = true);
    assert_eq!(
        first_break(
            "v2-w0-018",
            NarratorLength::Standard,
            off,
            true,
            None,
            KNELT_TO_DRINK
        ),
        None
    );
    assert_eq!(
        first_break(
            "v2-w0-018",
            NarratorLength::Standard,
            on,
            true,
            None,
            KNELT_TO_DRINK
        ),
        Some(CheckReasonCode::TabooBroken)
    );
    // Naming water without drinking it does not break the taboo.
    assert_eq!(
        first_break(
            "v2-w0-018",
            NarratorLength::Standard,
            on,
            true,
            None,
            "The party rested under a cold sky."
        ),
        None
    );
}

#[test]
fn the_taboo_arm_rejects_night_travel_leaving_things_and_debts() {
    let on = arms(|a| a.taboo_rules = true);
    // v2-w0-009: a night turn, taboo "no travel at night", the party did not travel.
    assert_eq!(
        first_break(
            "v2-w0-009",
            NarratorLength::Standard,
            on,
            true,
            None,
            "The wagon rolls on through the dark."
        ),
        Some(CheckReasonCode::TabooBroken)
    );
    assert_eq!(
        first_break(
            "v2-w0-009",
            NarratorLength::Standard,
            on,
            true,
            None,
            "The camp keeps still through the dark."
        ),
        None
    );
    // v2-w0-010: a rest, taboo "leave nothing behind".
    assert_eq!(
        first_break(
            "v2-w0-010",
            NarratorLength::Standard,
            on,
            true,
            None,
            "A worn crate is left behind at the bend."
        ),
        Some(CheckReasonCode::TabooBroken)
    );
    // v2-w0-001: doctrine "no debts"; the turn names no debt.
    assert_eq!(
        first_break(
            "v2-w0-001",
            NarratorLength::Standard,
            on,
            false,
            None,
            "The travellers owe a stranger a kindness."
        ),
        Some(CheckReasonCode::TabooBroken)
    );
}

#[test]
fn the_taboo_arm_rejects_the_taboo_stated_as_filler() {
    let on = arms(|a| a.taboo_rules = true);
    // v2-w0-018: nothing on the turn touches a river; stating the taboo is filler.
    assert_eq!(
        first_break(
            "v2-w0-018",
            NarratorLength::Standard,
            on,
            true,
            None,
            "The party rested. No river water was taken."
        ),
        Some(CheckReasonCode::TabooAsFiller)
    );
}

#[test]
fn the_water_arm_rejects_a_river_the_turn_does_not_provide() {
    let on = arms(|a| a.no_water_source = true);
    // v2-w0-005: Crow's Perch, plains, water not gained, themes hunger and commerce: no source.
    let line = "The party camps by the riverbank in the cold.";
    assert_eq!(
        first_break("v2-w0-005", NarratorLength::Standard, on, true, None, line),
        Some(CheckReasonCode::InventedWaterSource)
    );
    assert_eq!(
        first_break(
            "v2-w0-005",
            NarratorLength::Standard,
            ExperimentArms::default(),
            true,
            None,
            line
        ),
        None
    );
    // v2-w0-007: water was gained, so it came from somewhere.
    assert_eq!(
        first_break(
            "v2-w0-007",
            NarratorLength::Standard,
            on,
            false,
            None,
            "A stream runs clear below the bluffs."
        ),
        None
    );
    // A swamp has standing water; a crossing is over a river.
    assert_eq!(
        first_break(
            "v2-w0-001",
            NarratorLength::Standard,
            on,
            false,
            None,
            "A grey pond steams in the swamp."
        ),
        None
    );
    assert!(
        etv_core::narrator_truth::place_water_words("Stillwater Crossing")
            .contains(&"river".to_string())
    );
}

#[test]
fn a_river_theme_gives_river_scenery_but_the_taboo_still_forbids_drinking_from_it() {
    // v2-w0-018 carries the run theme "river" and the taboo "no river water".
    assert!(turn("v2-w0-018")
        .context
        .themes()
        .iter()
        .any(|t| t == "river"));
    let water = arms(|a| a.no_water_source = true);
    let both = arms(|a| {
        a.no_water_source = true;
        a.taboo_rules = true;
    });
    let scenery = "The party rested by the riverbank in the cold.";
    assert_eq!(
        first_break(
            "v2-w0-018",
            NarratorLength::Standard,
            water,
            true,
            None,
            scenery
        ),
        None
    );
    assert_eq!(
        first_break(
            "v2-w0-018",
            NarratorLength::Standard,
            both,
            true,
            None,
            scenery
        ),
        None
    );
    assert_eq!(
        first_break(
            "v2-w0-018",
            NarratorLength::Standard,
            both,
            true,
            None,
            KNELT_TO_DRINK
        ),
        Some(CheckReasonCode::TabooBroken)
    );
}
