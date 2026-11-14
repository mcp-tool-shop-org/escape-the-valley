//! The player is told what is actually wrong with the narrator: a cloud key that cannot be read,
//! a build without the cloud narrator, or a build without any narrator connection, each in its
//! own words. The reason goes in the session's lines (line mode prints them, the full-screen
//! interface logs them), never straight to the terminal. A narrator that is switched off looks
//! up no key at all. (Sequence A, a settings file that fails validation, is covered in
//! `phase3_settings_disclosure.rs`.)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use etv_core::cloud_claude::{HttpReply, HttpsPost};
use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::narrator::{FallbackPhraseTable, NarratorSettings, NullNarrator, TransportError};
use etv_core::narrator_provider::{KeySource, NarratorProvider};
use etv_core::play::{
    connect_narrator_with, narrator_transport, NARRATOR_OFF_LINE, NO_NARRATOR_CONNECTION_LINE,
};
use etv_core::tui::{App, Model, Reconnected, ViewData};
use etv_core::worldgen::create_new_run_with_content;

/// An HTTPS client that must never be reached in these tests.
struct Unreachable(Arc<AtomicUsize>);

impl HttpsPost for Unreachable {
    fn post(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &str,
        _timeout: Duration,
    ) -> Result<HttpReply, TransportError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(TransportError::ConnectionFailed("not in tests".to_string()))
    }
}

/// Sequence B's settings: Claude, with the key in a variable that is not set.
fn cloud_settings() -> NarratorSettings {
    NarratorSettings {
        provider: NarratorProvider::Claude,
        model: "claude-haiku-5-5".to_string(),
        key: KeySource {
            env: Some("ETV_UNSET".to_string()),
            file: None,
        },
        ..NarratorSettings::default()
    }
}

fn no_variables(_: &str) -> Option<String> {
    None
}

fn unreachable_http(posts: &Arc<AtomicUsize>) -> Option<Box<dyn HttpsPost>> {
    Some(Box::new(Unreachable(Arc::clone(posts))))
}

#[test]
fn a_missing_cloud_key_says_so_and_not_that_the_build_has_no_connection() {
    let posts = Arc::new(AtomicUsize::new(0));
    let settings = cloud_settings();
    let connection = connect_narrator_with(&settings, |s| {
        narrator_transport(s, None, unreachable_http(&posts), &no_variables)
    });
    assert!(connection.narrator.is_none());
    assert_eq!(
        connection.lines,
        vec![
            "The cloud narrator could not start: the environment variable named in the settings is not set."
                .to_string(),
            NARRATOR_OFF_LINE.to_string(),
        ]
    );
    assert!(!connection
        .lines
        .iter()
        .any(|l| l == NO_NARRATOR_CONNECTION_LINE));
    assert_eq!(posts.load(Ordering::SeqCst), 0, "no request without a key");
}

#[test]
fn each_build_without_a_connection_says_which_one_it_lacks() {
    // A build without the cloud narrator, asked for a cloud provider.
    let err = narrator_transport(&cloud_settings(), None, None, &no_variables)
        .err()
        .unwrap();
    assert_eq!(
        err,
        "This build has no cloud narrator, so the claude provider in the settings cannot be used."
    );
    // A build with no narrator connection at all, asked for the local narrator: only this one
    // says "no narrator connection".
    let local = NarratorSettings::default();
    let err = narrator_transport(&local, None, None, &no_variables)
        .err()
        .unwrap();
    assert_eq!(err, NO_NARRATOR_CONNECTION_LINE);
}

#[test]
fn a_narrator_that_is_off_looks_up_no_key() {
    let built = AtomicUsize::new(0);
    let looked_up = AtomicUsize::new(0);
    let lookup = |_: &str| {
        looked_up.fetch_add(1, Ordering::SeqCst);
        None
    };
    let settings = NarratorSettings {
        enabled: false,
        ..cloud_settings()
    };
    let connection = connect_narrator_with(&settings, |s| {
        built.fetch_add(1, Ordering::SeqCst);
        narrator_transport(s, None, None, &lookup)
    });
    assert_eq!(built.load(Ordering::SeqCst), 0, "no transport was built");
    assert_eq!(looked_up.load(Ordering::SeqCst), 0, "no key was looked up");
    assert_eq!(
        connection.lines,
        vec![
            "The narrator is switched off in the settings.".to_string(),
            NARRATOR_OFF_LINE.to_string(),
        ]
    );
}

fn table() -> &'static FallbackPhraseTable {
    static TABLE: OnceLock<FallbackPhraseTable> = OnceLock::new();
    TABLE.get_or_init(FallbackPhraseTable::embedded)
}

#[test]
fn a_reconnect_in_the_interface_logs_the_reason_instead_of_writing_to_the_screen() {
    // In the full-screen interface, saving the settings reconnects the narrator while the raw
    // screen is up. The reason must reach the log, where the player reads it.
    let posts = Arc::new(AtomicUsize::new(0));
    let content = embedded_content_pack();
    let engine = StepEngine::with_content(create_new_run_with_content(42, content), content);
    let model = Model::new(
        ViewData::from_engine(&engine),
        cloud_settings(),
        "TEST",
        "HASH",
        Vec::new(),
        false,
    );
    let dir = tempfile::tempdir().unwrap();
    let posts_for_hook = Arc::clone(&posts);
    let mut app = App::new(
        model,
        engine,
        &NullNarrator,
        Some(table()),
        dir.path().into(),
        false,
    )
    .with_reconnect(move |settings: &NarratorSettings| {
        let connection = connect_narrator_with(settings, |s| {
            narrator_transport(s, None, unreachable_http(&posts_for_hook), &no_variables)
        });
        Reconnected {
            narrator: std::rc::Rc::new(NullNarrator),
            live: connection.narrator.is_some(),
            lines: connection.lines,
            summary: None,
        }
    });
    app.press_code(crossterm::event::KeyCode::Enter);
    app.press_char('o');
    app.press_char('2'); // the profile: a change, so saving reconnects
    app.press_char('s');
    assert_eq!(app.model.notice.as_deref(), Some("Settings saved."));
    let log = app.model.log.join("\n");
    assert!(
        log.contains(
            "The cloud narrator could not start: the environment variable named in the settings is not set."
        ),
        "{log}"
    );
    assert!(!log.contains(NO_NARRATOR_CONNECTION_LINE), "{log}");
    assert_eq!(posts.load(Ordering::SeqCst), 0);
}
