//! "Says what happened": rules that test a narrator line against its turn's facts, the outcome
//! prompt's lead, the repeated-opening and event-copying rules, and how the harness scores them.
//! All of it is off in the game and switched on only as an experiment arm.
//!
//! The lines are real: they were written by the model in the combination arm of session 3 and
//! accepted by the checker as it stood. The numbers in the test names are the line numbers of
//! the reading sample made from that run.
//!
//! No model is called anywhere in this file.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::measure::{
    parse_any_turn_set, parse_turn_set_v2, run_cli, run_measurement_any, MeasureConfig,
    MeasureReport, TurnRow, TurnSpecV2,
};
use etv_core::measure_stats::{content_words, RowsSummary};
use etv_core::model::GMProfile;
use etv_core::narrator::{
    build_outcome_prompts, build_scene_prompts, check_generated_line, CheckReasonCode,
    CheckerContext, NarratorLength, NarratorTransport, TransportError,
};
use etv_core::narrator_experiment::{
    checker_context_for, lead_outcome_system_prompt, rulings_system_prompt, ExperimentArms,
    OUTCOME_LEAD_HEADING,
};
use etv_core::narrator_truth::{
    opening_two_words, opens_with_event_text, says_what_happened_failures, truth_failures,
    what_happened, TruthFacts,
};
use serde_json::{json, Value};

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

/// The "says what happened" codes a line breaks on a turn, for its outcome or scene slot.
fn swh(id: &str, line: &str, is_outcome: bool) -> Vec<CheckReasonCode> {
    let facts = TruthFacts::from_context(&turn(id).context, is_outcome);
    says_what_happened_failures(line, &facts)
        .into_iter()
        .map(|f| f.code)
        .collect()
}

// ---- the stored context: which supplies stand at zero -------------------------------------------

#[test]
fn every_turn_stores_the_supplies_that_stand_at_zero() {
    // v2-w0-025: food ran out long ago; water was gained on the turn.
    let t = turn("v2-w0-025");
    assert_eq!(
        t.context.supplies_at_zero(),
        [
            "ammo",
            "cloth",
            "firewood",
            "food",
            "lantern_oil",
            "parts",
            "tools"
        ]
    );
    assert_eq!(t.context.shown_deltas()["water"], 7);
    // v2-w0-010: a rest; firewood was spent, and some is left.
    let t = turn("v2-w0-010");
    assert_eq!(t.context.supplies_at_zero(), ["lantern_oil"]);
    assert_eq!(t.context.shown_deltas()["firewood"], -1);
    // Water is not at zero on any turn of the set, so no line of it may say there is none.
    let set = parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap();
    assert!(set
        .turns
        .iter()
        .all(|t| !t.context.supplies_at_zero().iter().any(|k| k == "water")));
    // The list is sorted and holds no key twice.
    for t in &set.turns {
        let keys = t.context.supplies_at_zero();
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{}", t.id);
    }
}

// ---- 1a and 1b: supplies -------------------------------------------------------------------------

/// Sample line 4 (pyrrhic ending, v2-w0-025): water was GAINED on the turn.
#[test]
fn line_4_no_water_on_a_turn_that_gained_water_fails() {
    let line = "Mud swells. Breath falters. No water. No food.";
    let facts = TruthFacts::from_context(&turn("v2-w0-025").context, true);
    let failures = says_what_happened_failures(line, &facts);
    assert_eq!(failures[0].code, CheckReasonCode::SupplyStateContradiction);
    assert_eq!(
        failures[0].detail,
        "says there is no 'water', but water is not at zero"
    );
    // "No food." alone is true on that turn: food is at zero.
    assert!(
        !swh("v2-w0-025", "No food.", false).contains(&CheckReasonCode::SupplyStateContradiction)
    );
}

/// Sample lines 1, 6 and 29: a supply was spent, not gone.
#[test]
fn spent_is_not_gone() {
    // 1 (v2-w0-001): water spent, not at zero.
    assert!(swh(
        "v2-w0-001",
        "Compass spins. Sun hides. Mud swells. No water. No food. No way.",
        true
    )
    .contains(&CheckReasonCode::SupplyStateContradiction));
    // 6 (v2-w0-010): firewood spent, not at zero.
    assert!(swh(
        "v2-w0-010",
        "Mud sucks at the heels. Firewood gone. Night deepens.",
        true
    )
    .contains(&CheckReasonCode::SupplyStateContradiction));
    // 29 (v2-w0-022): "Water is gone from the canteen".
    assert!(swh(
        "v2-w0-022",
        "Sand shifts underfoot. Water is gone from the canteen, yet the vessel feels full.",
        true
    )
    .contains(&CheckReasonCode::SupplyStateContradiction));
    // "Out of" is read the same way.
    assert!(swh("v2-w0-022", "The party is out of water.", false)
        .contains(&CheckReasonCode::SupplyStateContradiction));
    // Saying a supply was spent is fine when it was spent.
    assert_eq!(
        swh("v2-w0-010", "Firewood spent. Night deepens.", false),
        vec![]
    );
    // And "no" before something that is not a supply is not this rule's business.
    assert_eq!(swh("v2-w0-010", "No sound. No wind.", false), vec![]);
}

#[test]
fn a_supply_that_was_gained_is_not_described_as_lost() {
    // v2-w0-025 gained water.
    for line in [
        "Water spent at the well.",
        "The water is lost.",
        "Water runs dry.",
    ] {
        assert_eq!(
            swh("v2-w0-025", line, false),
            vec![CheckReasonCode::SupplyStateContradiction],
            "{line}"
        );
    }
    // Sample line 7's shape: one supply spent, another gained, each said truly.
    assert_eq!(
        swh("v2-w0-025", "Water gained. The road was long.", false),
        vec![]
    );
}

// ---- 1c: endings and deaths ----------------------------------------------------------------------

/// Sample line 22 (lost ending, v2-w0-075). Its last sentence must pass.
#[test]
fn line_22_no_one_left_passes_on_a_lost_ending() {
    assert_eq!(swh("v2-w0-075", "No one left.", true), vec![]);
    assert_eq!(
        swh(
            "v2-w0-075",
            "Bear prints stretch wide in the mud. No sound. No one left.",
            true
        ),
        vec![]
    );
    // The whole of line 22 also says "No water.", and water was spent, not gone, on that turn.
    // That one sentence is what fails; "No one left." is not.
    assert_eq!(
        swh(
            "v2-w0-075",
            "Bear prints stretch wide in the mud. Earth pulls back like breath. No water. No sound. No one left.",
            true
        ),
        vec![CheckReasonCode::SupplyStateContradiction]
    );
}

#[test]
fn an_ending_turns_outcome_line_says_the_journey_ended() {
    // Sample line 3 (triumphant, v2-w0-023): mud, wind and a track.
    let line_3 = "Mud sucks at the heels. Wind stings the face. A track cuts through the grass, not made by foot.";
    assert_eq!(
        swh("v2-w0-023", line_3, true),
        vec![CheckReasonCode::EndingNotConveyed]
    );
    assert_eq!(
        swh(
            "v2-w0-023",
            "The wagon reaches the town at dusk. The trail ends at its gate.",
            true
        ),
        vec![]
    );
    // The scene line of the same turn is not asked to carry the ending.
    assert_eq!(swh("v2-w0-023", line_3, false), vec![]);

    // Sample line 14 (pyrrhic, v2-w1-022).
    assert_eq!(
        swh(
            "v2-w1-022",
            "Oxen lag in mud. Smoke curls from the wagon. A river glints in the trees.",
            true
        ),
        vec![CheckReasonCode::EndingNotConveyed]
    );
    assert_eq!(
        swh(
            "v2-w1-022",
            "The wagon arrives with fewer than set out.",
            true
        ),
        vec![]
    );

    // Sample line 12 (lost, v2-w0-047): the oxen drag the wagon with no one left.
    let line_12 = "The oxen drag the wagon through thick reeds, mud sucking at the wheels. The trail ends where the water has no name.";
    assert_eq!(
        swh("v2-w0-047", line_12, true),
        vec![CheckReasonCode::ActionWithNoOneLeft]
    );
    // Sample line 2 (lost, v2-w0-004) conveys it with "No one moves".
    assert!(!swh(
        "v2-w0-004",
        "Dark clouds gather. Wind lashes the mud. No one moves.",
        true
    )
    .contains(&CheckReasonCode::EndingNotConveyed));
}

#[test]
fn a_death_turns_outcome_line_says_someone_died() {
    // Sample line 11 (death, v2-w0-002): the event text and a child's grip.
    assert_eq!(
        swh(
            "v2-w0-002",
            "Small hands reach out beside the rutted path. A child's grip tightens on the wagon rim.",
            true
        ),
        vec![CheckReasonCode::DeathNotConveyed]
    );
    assert_eq!(
        swh(
            "v2-w0-002",
            "A traveller died before the camp was struck.",
            true
        ),
        vec![]
    );
    assert_eq!(
        swh("v2-w0-002", "A traveller does not rise at dawn.", true),
        vec![]
    );
}

// ---- 1d: arrivals --------------------------------------------------------------------------------

#[test]
fn an_arrival_turns_outcome_line_names_arriving_or_the_place() {
    // Sample line 5 (v2-w0-015, arrived at The Scar): mud, wind and a hum.
    assert_eq!(
        swh(
            "v2-w0-015",
            "Mud sucks at the wheels. Wind bites the face. The trail hums with a low, steady sound.",
            true
        ),
        vec![CheckReasonCode::ArrivalNotConveyed]
    );
    assert_eq!(turn("v2-w0-015").context.place(), "The Scar");
    // The place's name is enough, and so is an arrival verb.
    assert_eq!(
        swh("v2-w0-015", "The Scar rises ahead, grey and steep.", true),
        vec![]
    );
    assert_eq!(
        swh("v2-w0-015", "The wagon reaches a settlement at dusk.", true),
        vec![]
    );
    // An ordinary travel turn is asked for nothing.
    assert_eq!(
        swh(
            "v2-w0-006",
            "Fire glows low. Smoke curls into the dark. A path splits ahead.",
            true
        ),
        vec![]
    );
}

// ---- 1e: no one left -----------------------------------------------------------------------------

#[test]
fn with_no_one_left_the_party_does_nothing() {
    for line in [
        "The oxen drag the wagon through thick reeds.",
        "The travellers push on toward the ridge.",
        "They make camp by the water.",
    ] {
        assert!(
            swh("v2-w0-047", line, false).contains(&CheckReasonCode::ActionWithNoOneLeft),
            "{line}"
        );
    }
    for line in [
        "No one moves.",
        "The wagon stands in the reeds.",
        "The oxen stand unattended, still in their yoke.",
        "The last traveller lies by the wheel.",
    ] {
        assert!(
            !swh("v2-w0-047", line, false).contains(&CheckReasonCode::ActionWithNoOneLeft),
            "{line}"
        );
    }
    // With someone alive the same line is fine.
    assert_eq!(
        swh(
            "v2-w0-006",
            "The oxen drag the wagon through thick reeds.",
            false
        ),
        vec![]
    );
}

// ---- the lines that read well still pass ----------------------------------------------------------

/// Sample lines 10, 17 and 26, which read well against their facts.
#[test]
fn the_lines_that_read_well_pass() {
    assert_eq!(
        swh(
            "v2-w0-006",
            "Fire glows low. Smoke curls into the dark. A path splits ahead.",
            true
        ),
        vec![]
    );
    assert_eq!(
        swh(
            "v2-w0-020",
            "The oxen slow in the dust, their breath puffing in the heat. The town stands silent, fingers pressed to the gate.",
            true
        ),
        // Line 17 mentions the town but uses no arrival word and not the place's name.
        vec![CheckReasonCode::ArrivalNotConveyed]
    );
    assert_eq!(
        swh(
            "v2-w0-020",
            "The oxen slow in the dust as the wagon reaches the town. Its people stand silent at the gate.",
            true
        ),
        vec![]
    );
}

// ---- event copying and repeated openings ----------------------------------------------------------

#[test]
fn a_line_may_not_open_with_five_words_of_the_event_text() {
    // Sample lines 11 and 19: the event text, word for word.
    let event = "Small hands reach out beside the rutted path.";
    assert!(opens_with_event_text(
        "Small hands reach out beside the rutted path.",
        event
    ));
    assert!(opens_with_event_text(
        "Small hands reach out beside the rutted path. A child's grip tightens on the wagon rim.",
        event
    ));
    // Sample line 21: "The fire has died. The cold seeps..." against "The fire has died. The cold presses in."
    assert!(opens_with_event_text(
        "The fire has died. The cold seeps through the mud.",
        "The fire has died. The cold presses in. No firewood remains."
    ));
    // Four words in common is allowed, and so is the event's text later in the line.
    assert!(!opens_with_event_text(
        "Small hands reach out from the reeds.",
        event
    ));
    assert!(!opens_with_event_text(
        "By the road, small hands reach out beside the rutted path.",
        event
    ));
    assert!(!opens_with_event_text("Mud grips the wheels.", ""));

    // Through the checker, with the arm on and off.
    let ctx = turn("v2-w0-002").context;
    let arms = ExperimentArms {
        no_event_copy: true,
        ..ExperimentArms::default()
    };
    let line = "Small hands reach out beside the rutted path.";
    let on = checker_context_for(
        &ctx,
        NarratorLength::Standard,
        GMProfile::Fireside,
        arms,
        false,
        None,
    );
    assert_eq!(
        check_generated_line(line, &on).unwrap_err().code,
        CheckReasonCode::EventTextCopied
    );
    let off = checker_context_for(
        &ctx,
        NarratorLength::Standard,
        GMProfile::Fireside,
        ExperimentArms::default(),
        false,
        None,
    );
    assert_eq!(check_generated_line(line, &off), Ok(()));
    assert_eq!(
        off,
        CheckerContext::from_narration_context(&ctx, NarratorLength::Standard)
    );
}

#[test]
fn an_outcome_line_may_not_open_as_the_previous_accepted_line_did() {
    assert_eq!(opening_two_words("Mud grips the wheels."), "mud grips");
    assert_eq!(opening_two_words("Mud, grips the wheels."), "mud grips");
    assert_eq!(opening_two_words("Rain."), "rain");

    let ctx = turn("v2-w0-006").context;
    let arms = ExperimentArms {
        no_repeat_opening: true,
        ..ExperimentArms::default()
    };
    let check = |line: &str, is_outcome: bool, previous: Option<&str>| {
        let c = checker_context_for(
            &ctx,
            NarratorLength::Standard,
            GMProfile::Chronicler,
            arms,
            is_outcome,
            previous,
        );
        check_generated_line(line, &c).err().map(|f| f.code)
    };
    assert_eq!(
        check("Mud grips the wheels again.", true, Some("mud grips")),
        Some(CheckReasonCode::RepeatedOpening)
    );
    // A different second word is a different opening.
    assert_eq!(
        check("Mud holds the wheels.", true, Some("mud grips")),
        None
    );
    // The scene line is not held to it, and the first line of a run has nothing before it.
    assert_eq!(
        check("Mud grips the wheels again.", false, Some("mud grips")),
        None
    );
    assert_eq!(check("Mud grips the wheels again.", true, None), None);
}

// ---- the outcome prompt's lead --------------------------------------------------------------------

#[test]
fn what_happened_is_the_turns_one_main_fact() {
    let fact = |id: &str| what_happened(&turn(id).context);
    // An ending comes first, even though a traveller also died on v2-w0-004.
    assert_eq!(
        fact("v2-w0-004"),
        "The last of the party has died. The journey ends here."
    );
    assert_eq!(
        fact("v2-w0-023"),
        "The party reached Ravenmoor and the end of the trail. The journey is over."
    );
    assert_eq!(
        fact("v2-w0-025"),
        "The party reached Copperhill and the end of the trail, with some of its number lost on the way. The journey is over."
    );
    // Then a death (v2-w0-001), then an arrival (v2-w0-015).
    assert_eq!(fact("v2-w0-001"), "A traveller died.");
    assert_eq!(fact("v2-w0-015"), "The party arrived at The Scar.");
    // Then the choice taken (v2-w0-014), then the camp action (v2-w0-011 hunt, v2-w0-010 rest,
    // v2-w0-006 travel).
    assert_eq!(
        fact("v2-w0-014"),
        "The party chose: Give what you can spare."
    );
    assert_eq!(fact("v2-w0-011"), "The party hunted and brought back food.");
    assert_eq!(fact("v2-w0-010"), "The party rested.");
    assert_eq!(fact("v2-w0-006"), "The party travelled on.");
}

#[test]
fn the_outcome_lead_arm_puts_the_fact_first_and_drops_the_event_text() {
    let ctx = turn("v2-w0-002").context;
    let (system, _) = build_outcome_prompts(GMProfile::Fireside, NarratorLength::Standard, &ctx);
    assert!(system.contains("- event_text: Small hands reach out beside the rutted path."));
    let lead = lead_outcome_system_prompt(&system, &ctx);
    let head = format!("{OUTCOME_LEAD_HEADING}\n- A traveller died.\n\nTURN FACTS:\n");
    assert_eq!(lead.matches(&head).count(), 1, "{lead}");
    assert!(!lead.contains("- event_text:"));
    assert!(!lead.contains("- choices_offered:"));
    // Everything else of the prompt is kept, in order.
    assert!(lead.contains("- event_title: Hungry Family"));
    assert!(lead.contains("WHAT RESULTED:"));
    assert!(lead.find(OUTCOME_LEAD_HEADING).unwrap() < lead.find("TURN FACTS:").unwrap());
    assert!(
        lead.find("SHARED VOICE HARD RULES:").unwrap() < lead.find(OUTCOME_LEAD_HEADING).unwrap()
    );
    // The scene call is not touched by this arm (checked through the harness below).
    let (scene, _) = build_scene_prompts(GMProfile::Fireside, NarratorLength::Standard, &ctx);
    assert!(scene.contains("- event_text: Small hands reach out beside the rutted path."));
}

#[test]
fn fireside_is_told_the_team_is_oxen_only_if_it_is_mentioned() {
    let ctx = turn("v2-w0-006").context;
    let (system, _) = build_outcome_prompts(GMProfile::Fireside, NarratorLength::Standard, &ctx);
    let arm = rulings_system_prompt(&system, GMProfile::Fireside, NarratorLength::Standard);
    assert!(arm.contains("- If the team is mentioned, it is oxen."));
    assert!(!arm.contains("The wagon is drawn by oxen"));
}

// ---- through the harness --------------------------------------------------------------------------

struct Script {
    answers: Vec<&'static str>,
    bodies: Mutex<Vec<String>>,
}

impl Script {
    fn new(answers: &[&'static str]) -> Arc<Self> {
        Arc::new(Self {
            answers: answers.to_vec(),
            bodies: Mutex::new(Vec::new()),
        })
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|b| serde_json::from_str(b).unwrap())
            .collect()
    }
}

impl NarratorTransport for Script {
    fn get_tags(&self, _e: &str, _t: Duration) -> Result<(u16, String), TransportError> {
        Ok((
            200,
            r#"{"models":[{"name":"fixture-model:latest"}]}"#.to_string(),
        ))
    }

    fn post_generate(
        &self,
        _e: &str,
        body_json: &str,
        _t: Duration,
    ) -> Result<(u16, String), TransportError> {
        if body_json.contains("\"Warm up.\"") {
            return Ok((200, r#"{"response":""}"#.to_string()));
        }
        let mut bodies = self.bodies.lock().unwrap();
        let answer = self.answers[bodies.len().min(self.answers.len() - 1)];
        bodies.push(body_json.to_string());
        Ok((200, json!({ "response": answer }).to_string()))
    }
}

fn cell(profile: GMProfile, turns: usize) -> MeasureConfig {
    let mut cfg = MeasureConfig::new("fixture-model");
    cfg.profiles = vec![profile];
    cfg.lengths = vec![NarratorLength::Standard];
    cfg.max_turns = Some(turns);
    cfg
}

fn run(cfg: &MeasureConfig, answers: &[&'static str]) -> (MeasureReport, Vec<TurnRow>, Vec<Value>) {
    let set = parse_any_turn_set(V2_BYTES, V2_SHA256).unwrap();
    let transport = Script::new(answers);
    let (report, rows) = run_measurement_any(&set, V2_SHA256, cfg, transport.clone()).unwrap();
    (report, rows, transport.bodies())
}

const NO_WATER: &str = r#"{"sentences":["Mud swells. No water."]}"#;
const DIED: &str = r#"{"sentences":["A traveller died before the camp was struck."]}"#;
const MUD: &str = r#"{"sentences":["Mud grips the wheels."]}"#;

/// The first turn of the set (v2-w0-001) is a death turn where water was spent, not gone.
#[test]
fn both_rates_are_reported_and_an_old_run_is_scored_without_the_arm() {
    // Arm off: "No water." passes the rules as they stand. The run is still scored for whether
    // its accepted lines say what happened.
    let cfg = cell(GMProfile::Chronicler, 1);
    let (report, rows, _) = run(&cfg, &[NO_WATER]);
    let c = &report.cells[0];
    assert_eq!((c.slots, c.accepted_slots), (2, 2));
    assert_eq!(
        (c.says_what_happened_pass, c.says_what_happened_rate),
        (0, 0.0)
    );
    assert_eq!(c.good_line_rate, 0.0);
    assert_eq!(
        c.says_what_happened_fail_by_code,
        [
            // The outcome line also fails to say that a traveller died.
            ("death_not_conveyed".to_string(), 1),
            ("supply_state_contradiction".to_string(), 2),
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(rows[0].scene.status, "accepted");
    assert_eq!(
        rows[0].scene.truth_breaks[0].code,
        "supply_state_contradiction"
    );
    assert_eq!(rows[0].run_key, "w0-cautious-1");

    // Arm on: the same answer is rejected, and a line that says it is accepted.
    let mut on = cfg.clone();
    on.experiment.says_what_happened = true;
    let (report, rows, _) = run(&on, &[NO_WATER]);
    assert_eq!(report.experiment, "says-what-happened");
    assert_eq!(report.cells[0].accepted_slots, 0);
    assert_eq!(
        report.cells[0].rejects_by_code["supply_state_contradiction"],
        2
    );
    assert_eq!(
        rows[0].outcome.reject_detail.as_deref(),
        Some("says there is no 'water', but water is not at zero")
    );
    let (report, _, _) = run(&on, &[DIED]);
    let c = &report.cells[0];
    assert_eq!((c.accepted_slots, c.says_what_happened_pass), (2, 2));
    assert_eq!((c.says_what_happened_rate, c.good_line_rate), (1.0, 1.0));
}

#[test]
fn the_repeated_opening_arm_retries_and_forgets_between_runs() {
    // Turns 1 to 4 of the set are one run (seed 1); turn 5 starts the next (seed 2).
    let mut cfg = cell(GMProfile::Chronicler, 5);
    cfg.experiment.no_repeat_opening = true;
    cfg.experiment.candidates = 2;
    // Turn 1: scene "Mud grips..." accepted; outcome "Mud grips..." repeats it, so a second
    // candidate is asked for and "Wind rises..." is accepted. And so on.
    let wind: &'static str = r#"{"sentences":["Wind rises over the ridge."]}"#;
    let (report, rows, _) = run(&cfg, &[MUD, MUD, wind, MUD, MUD, wind]);
    assert_eq!(report.experiment, "candidates-2+no-repeat-opening");
    assert_eq!(rows[0].scene.attempts, 1);
    assert_eq!(rows[0].outcome.attempts, 2);
    assert_eq!(
        rows[0].outcome.attempt_outcomes,
        ["rule_reject:repeated_opening", "accepted"]
    );
    assert_eq!(
        rows[0].outcome.candidate.as_deref(),
        Some("Wind rises over the ridge.")
    );
    // The run key changes at the fifth turn.
    assert_eq!(rows[3].run_key, "w0-cautious-1");
    assert_eq!(rows[4].run_key, "w0-cautious-2");

    // With the arm off the repeats are accepted, and the report counts them: within a run, an
    // accepted outcome line that opens as the accepted line before it did.
    let off = cell(GMProfile::Chronicler, 5);
    let (report, rows, _) = run(&off, &[MUD]);
    assert_eq!(report.cells[0].accepted_slots, 10);
    assert!(rows.iter().all(|r| r.outcome.attempts == 1));
    // Five outcome lines each repeat their own turn's scene line.
    assert_eq!(report.cells[0].accepted_repeating_opening, 5);
}

#[test]
fn the_harness_measures_oxen_and_content_words_and_copying() {
    assert_eq!(
        content_words("The oxen drag the wagon through thick reeds, mud sucking at the wheels."),
        ["drag", "oxen", "reeds", "sucking", "thick", "wagon", "wheels"]
    );
    let oxen: &'static str = r#"{"sentences":["The oxen slow in the dust."]}"#;
    let mut cfg = cell(GMProfile::Fireside, 2);
    cfg.experiment.rulings_prompt = true;
    let (report, rows, _) = run(&cfg, &[oxen, MUD, oxen, oxen]);
    let c = &report.cells[0];
    assert_eq!(c.accepted_lines, 4);
    // Three of the four accepted lines mention the oxen.
    assert_eq!(c.oxen_share, 0.75);
    let oxen_count = c
        .top_content_words
        .iter()
        .find(|w| w.word == "oxen")
        .map(|w| w.lines);
    assert_eq!(oxen_count, Some(3));
    // The all-rules list is made with the arm's own checker: Fireside may say "oxen" here, so
    // no accepted line is listed as breaking the draft-animal rule.
    for row in &rows {
        for slot in [&row.scene, &row.outcome] {
            assert!(
                slot.all_rule_breaks
                    .iter()
                    .all(|b| b.code != "draft_animal_species"),
                "{:?}",
                slot.all_rule_breaks
            );
        }
    }
    assert!(!c.rejects_all_rules.contains_key("draft_animal_species"));

    // Copying: the second turn of the set (v2-w0-002) has the event text below.
    let copy: &'static str = r#"{"sentences":["Small hands reach out beside the rutted path."]}"#;
    let (report, rows, _) = run(&cell(GMProfile::Chronicler, 2), &[MUD, MUD, copy, copy]);
    assert_eq!(report.cells[0].accepted_copying_event_text, 2);
    assert!(rows[1].scene.copies_event_text);
    assert!(!rows[0].scene.copies_event_text);
}

#[test]
fn the_command_line_takes_the_new_arms_and_the_layer_count() {
    let set_path = format!(
        "{}/tests/fixtures/narrator_turn_set_v2.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let dir = std::env::temp_dir().join(format!("etv-swh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rows_path = dir.join("rows.jsonl");
    let args = |extra: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = [
            "--turn-set",
            &set_path,
            "--turn-set-sha256",
            V2_SHA256,
            "--profiles",
            "chronicler",
            "--lengths",
            "standard",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    };
    let transport = Script::new(&[DIED]);
    let mut out = Vec::new();
    run_cli(
        &args(&[
            "--model",
            "fixture-model",
            "--max-turns",
            "1",
            "--arm",
            "says-what-happened",
            "--arm",
            "outcome-lead",
            "--arm",
            "no-repeat-opening",
            "--arm",
            "no-event-copy",
            "--num-gpu",
            "18",
            "--rows",
            rows_path.to_str().unwrap(),
        ]),
        transport.clone(),
        &mut out,
    )
    .unwrap();
    let report: MeasureReport = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        report.experiment,
        "says-what-happened+outcome-lead+no-repeat-opening+no-event-copy+num-gpu-18"
    );
    let bodies = transport.bodies();
    // The layer count goes with every call; the lead is in the outcome call only.
    assert_eq!(bodies[0]["options"]["num_gpu"], json!(18));
    assert_eq!(bodies[1]["options"]["num_gpu"], json!(18));
    let scene_system = bodies[0]["system"].as_str().unwrap();
    assert!(!scene_system.contains(OUTCOME_LEAD_HEADING));
    assert!(scene_system.contains("- event_text: "));
    // The scene line "A traveller died..." is accepted; the outcome line opens the same way, so
    // the repeated-opening rule turns it down.
    assert_eq!(report.cells[0].accepted_slots, 1);
    assert_eq!(report.cells[0].rejects_by_code["repeated_opening"], 1);
    let outcome_system = bodies.last().unwrap()["system"].as_str().unwrap();
    assert!(outcome_system.contains(&format!("{OUTCOME_LEAD_HEADING}\n- A traveller died.")));
    assert!(!outcome_system.contains("- event_text: "));

    // Re-scoring the written rows calls no model and gives both rates.
    struct NoCalls;
    impl NarratorTransport for NoCalls {
        fn get_tags(&self, _e: &str, _t: Duration) -> Result<(u16, String), TransportError> {
            panic!("asked for the model list");
        }
        fn post_generate(
            &self,
            _e: &str,
            _b: &str,
            _t: Duration,
        ) -> Result<(u16, String), TransportError> {
            panic!("called the model");
        }
    }
    let mut out = Vec::new();
    run_cli(
        &args(&["--summarise-rows", rows_path.to_str().unwrap()]),
        Arc::new(NoCalls),
        &mut out,
    )
    .unwrap();
    let summary: RowsSummary = serde_json::from_slice(&out).unwrap();
    let c = &summary.cells[0];
    assert_eq!(
        (c.slots, c.accepted_slots, c.says_what_happened_pass),
        (2, 1, 1)
    );
    assert_eq!(c.says_what_happened_rate, 1.0);
    assert_eq!(c.good_line_rate, 0.5);

    let mut out = Vec::new();
    let err = run_cli(&args(&["--num-gpu", "many"]), Arc::new(NoCalls), &mut out).unwrap_err();
    assert_eq!(err, "--num-gpu needs a whole number");
    std::fs::remove_file(&rows_path).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

/// Rules 1a to 1e, the copying rule and the repeated opening are all off unless asked for.
#[test]
fn nothing_here_runs_in_the_game() {
    let ctx = turn("v2-w0-025").context;
    let shipped = CheckerContext::from_narration_context(&ctx, NarratorLength::Standard);
    assert_eq!(shipped.truth, None);
    assert_eq!(
        check_generated_line("Mud swells. No water. No food.", &shipped),
        Ok(())
    );
    let facts = TruthFacts::from_context(&ctx, true);
    assert!(!truth_failures("Mud swells. No water. No food.", &facts).is_empty());
    let arms = ExperimentArms::default();
    assert!(!arms.says_what_happened && !arms.outcome_lead);
    assert!(!arms.no_repeat_opening && !arms.no_event_copy);
    assert_eq!(arms.num_gpu, None);
    assert!(arms.is_shipped());
}
