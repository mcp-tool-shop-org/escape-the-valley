//! `balance_to_int` parses an IOU balance string taken from a ledger response, which is
//! untrusted input. It must return `Err` (never panic) for anything it cannot represent.
//!
//! Rule under test (doc comment on `balance_to_int`, from `_balance_to_int` in
//! `backpack.py`): "Parse an IOU balance string to an exact integer without binary floating
//! point. A non-integer balance is an error, not a truncation. Accepts an optional sign, a
//! decimal point and an exponent." The Python then does `int(Decimal(...))`; the Rust result
//! type is `i32`, so an integer value outside the i32 range is also `Err`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use etv_core::backpack::balance_to_int;

#[test]
fn review_inputs_return_err_not_panic() {
    // Review finding 3: these two panicked with "attempt to subtract with overflow" and
    // "attempt to negate with overflow" in a debug build.
    assert!(balance_to_int("1e-2147483648").is_err());
    assert!(balance_to_int("1.5e-2147483647").is_err());
}

#[test]
fn error_message_names_the_input_kind() {
    let msg = balance_to_int("1e-2147483648").unwrap_err();
    assert!(msg.starts_with("non-integer IOU balance"), "{msg}");
}

#[test]
fn ordinary_inputs_have_exact_values() {
    // (input, expected): worked by hand from the rule above.
    let table: &[(&str, Result<i32, ()>)] = &[
        ("0", Ok(0)),
        ("5", Ok(5)),
        ("-5", Ok(-5)),
        ("+7", Ok(7)),
        ("12.0", Ok(12)),
        ("12.000", Ok(12)),
        ("1.5e1", Ok(15)),
        ("1e3", Ok(1000)),
        ("1E3", Ok(1000)),
        ("150e-1", Ok(15)),
        ("0.0", Ok(0)),
        (" 42 ", Ok(42)),
        ("00012", Ok(12)),
        ("5.", Ok(5)),
        (".5e1", Ok(5)),
        ("2147483647", Ok(i32::MAX)),
        ("-2147483648", Ok(i32::MIN)),
        ("2147483648", Err(())),
        ("12.5", Err(())),
        ("0.5", Err(())),
        ("1e12", Err(())),
    ];
    for (input, want) in table {
        let got = balance_to_int(input);
        match want {
            Ok(v) => assert_eq!(got, Ok(*v), "input {input:?}"),
            Err(()) => assert!(got.is_err(), "input {input:?} gave {got:?}"),
        }
    }
}

#[test]
fn hostile_inputs_never_panic_and_have_correct_values() {
    let long_one = format!("1{}", "0".repeat(399));
    let long_zero = "0".repeat(400);
    let long_frac = format!("0.{}", "0".repeat(400));
    let long_frac_nz = format!("0.{}1", "0".repeat(400));
    let cases: Vec<(String, Result<i32, ()>)> = vec![
        ("1e2147483647".into(), Err(())),
        ("1e-2147483647".into(), Err(())),
        ("1e-9999999999999999999".into(), Err(())),
        ("1e9999999999999999999".into(), Err(())),
        ("1e+99999".into(), Err(())),
        (long_one, Err(())),
        (long_zero, Ok(0)),
        (long_frac, Ok(0)),
        (long_frac_nz, Err(())),
        ("9223372036854775807".into(), Err(())),
        ("9223372036854775808".into(), Err(())),
        ("-9223372036854775808".into(), Err(())),
        ("-9223372036854775809".into(), Err(())),
        ("-0".into(), Ok(0)),
        ("0e0".into(), Ok(0)),
        ("0e-2147483648".into(), Ok(0)),
        ("0e2147483647".into(), Ok(0)),
        ("0.0000e-2147483648".into(), Ok(0)),
        ("0.000e2147483647".into(), Ok(0)),
        ("".into(), Err(())),
        ("e5".into(), Err(())),
        ("1e".into(), Err(())),
        ("1e+".into(), Err(())),
        ("1e-".into(), Err(())),
        ("--1".into(), Err(())),
        ("+-1".into(), Err(())),
        ("1.2.3".into(), Err(())),
        ("NaN".into(), Err(())),
        ("inf".into(), Err(())),
        ("-Infinity".into(), Err(())),
        (" 1 ".into(), Ok(1)),
        ("\t1\n".into(), Ok(1)),
        ("1 2".into(), Err(())),
        (".".into(), Err(())),
        ("-.".into(), Err(())),
        (".e5".into(), Err(())),
        ("\u{0661}\u{0662}".into(), Err(())),
        ("1e\u{0665}".into(), Err(())),
        ("1e5e5".into(), Err(())),
    ];
    assert!(cases.len() >= 20);
    for (input, want) in &cases {
        let shown: String = input.chars().take(40).collect();
        let got = catch_unwind(AssertUnwindSafe(|| balance_to_int(input)))
            .unwrap_or_else(|_| panic!("panicked on input starting {shown:?}"));
        match want {
            Ok(v) => assert_eq!(got, Ok(*v), "input starting {shown:?}"),
            Err(()) => assert!(got.is_err(), "input starting {shown:?} gave {got:?}"),
        }
    }
}

#[test]
fn deterministic_sweep_never_panics() {
    const ALPHABET: &[u8] = b"0123456789.eE+-";
    let mut panics = 0usize;
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in 0..5000u64 {
        let len = (i % 25) as usize; // 0..=24 characters
        let mut s = String::new();
        for _ in 0..len {
            // Counter-driven mixing, no RNG crate.
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407 + i);
            s.push(ALPHABET[((state >> 33) % ALPHABET.len() as u64) as usize] as char);
        }
        if catch_unwind(AssertUnwindSafe(|| balance_to_int(&s))).is_err() {
            panics += 1;
        }
    }
    assert_eq!(panics, 0);
}
