//! Spike S3 — Kokoro v1.0 TTS (`bm_george`, speed `1.0`) in pure Rust via `ort` + `rodio` + `hound`
//! with Apache-2.0 Misaki lexicon (`us_gold`, `us_silver`, `gb_gold`, `gb_silver`) and BSD-2-Clause
//! CMUdict fallback. Links zero GPL (`espeak-ng` / `piper-phonemize`) symbols or libraries.

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ort::session::Session;
use ort::value::Tensor;
use serde::Deserialize;
use serde_json::Value;

const SAMPLE_RATE: u32 = 24_000;
const MAX_PHONEME_LEN: usize = 500;
const VOICE_NAME: &str = "bm_george";
const DEFAULT_SPEED: f32 = 1.0;

pub const GAME_LINES: [(&str, &str); 20] = [
    (
        "01",
        "Shallow Ford. The river runs low, but the mud pulls at the wheels with sullen persistence.",
    ),
    (
        "02",
        "Broken Axle. A crack like a gunshot. The wagon lurches sideways. The rear axle has split.",
    ),
    (
        "03",
        "Sudden Storm. Dark clouds mass without warning. The wind picks up sharply.",
    ),
    (
        "04",
        "Spoiled Rations. Something smells wrong in the provisions. Mold has taken hold.",
    ),
    (
        "05",
        "Dry Spring. The spring marked on your map is bone dry. Cracked mud and nothing else.",
    ),
    (
        "06",
        "Detour upstream for calmer waters. It costs 3 days and 12 lbs of salted provisions, but the wagon holds together.",
    ),
    (
        "07",
        "After 3 days on half rations, only 12 lbs of food and 4 gallons of clean water remain in the wagon.",
    ),
    (
        "08",
        "The party has arrived at Coldspring before nightfall, exhausted from the climb.",
    ),
    (
        "09",
        "Beyond Fellgate, the trail narrows between Thornhaven and the dry flats of Dryreach.",
    ),
    (
        "10",
        "Few travelers take the lower road from Dunhollow through the reeds of Whitmarsh.",
    ),
    (
        "11",
        "Elias and Martha steady the lead oxen while Ruth checks the frayed harness rope.",
    ),
    (
        "12",
        "Josiah is burning with fever. Agnes and Caleb watch the camp fire uneasily.",
    ),
    (
        "13",
        "Silas has died of exposure near Redwater. Hannah weeps beside the frostbitten trail.",
    ),
    (
        "14",
        "Warning. Barely one day of water remains, and you have no spare parts.",
    ),
    (
        "15",
        "Alert. One more break will cripple the wagon.",
    ),
    (
        "16",
        "Fallen Bridge. The old bridge lies in ruins, timbers scattered by some past flood or force.",
    ),
    (
        "17",
        "Lame Animal. One of the draft animals is favoring its left foreleg badly.",
    ),
    (
        "18",
        "Rockslide. Stones tumble from the slope above. The trail is half-buried.",
    ),
    (
        "19",
        "Lost Trail. The trail fades into unmarked ground. The way forward is unclear.",
    ),
    (
        "20",
        "Lanterns hang from the dead pines outside Millford, swaying though there is no wind. Somewhere beneath the wagon floorboards, three slow knocks answer the bells across the valley, and the oxen refuse to take another step.",
    ),
];

const CACHE_ASSETS: [(&str, &str); 8] = [
    (
        "kokoro-v1.0.onnx",
        "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx",
    ),
    (
        "voices-v1.0.bin",
        "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin",
    ),
    (
        "config.json",
        "https://huggingface.co/hexgrad/Kokoro-82M/raw/main/config.json",
    ),
    (
        "us_gold.json",
        "https://raw.githubusercontent.com/hexgrad/misaki/main/misaki/data/us_gold.json",
    ),
    (
        "us_silver.json",
        "https://raw.githubusercontent.com/hexgrad/misaki/main/misaki/data/us_silver.json",
    ),
    (
        "gb_gold.json",
        "https://raw.githubusercontent.com/hexgrad/misaki/main/misaki/data/gb_gold.json",
    ),
    (
        "gb_silver.json",
        "https://raw.githubusercontent.com/hexgrad/misaki/main/misaki/data/gb_silver.json",
    ),
    (
        "cmudict.dict",
        "https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict",
    ),
];

#[derive(Deserialize)]
struct KokoroConfig {
    vocab: HashMap<String, i64>,
}

struct MisakiLexicon {
    golds: HashMap<String, String>,
    silvers: HashMap<String, String>,
    cmu: HashMap<String, String>,
    vocab: HashMap<char, i64>,
}

impl MisakiLexicon {
    fn load(cache_dir: &Path) -> Result<Self, Box<dyn Error>> {
        let cfg_text = fs::read_to_string(cache_dir.join("config.json"))?;
        let cfg: KokoroConfig = serde_json::from_str(&cfg_text)?;
        let mut vocab = HashMap::new();
        for (k, v) in cfg.vocab {
            let mut chars = k.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                vocab.insert(c, v);
            }
        }

        let mut golds = HashMap::new();
        let mut silvers = HashMap::new();
        // Load US first so US entries always take precedence; GB fills only missing entries
        Self::insert_json_lexicon(&cache_dir.join("us_gold.json"), &mut golds, true)?;
        Self::insert_json_lexicon(&cache_dir.join("gb_gold.json"), &mut golds, false)?;
        Self::insert_json_lexicon(&cache_dir.join("us_silver.json"), &mut silvers, true)?;
        Self::insert_json_lexicon(&cache_dir.join("gb_silver.json"), &mut silvers, false)?;

        let cmu = Self::load_cmudict(&cache_dir.join("cmudict.dict"))?;

        Ok(Self {
            golds,
            silvers,
            cmu,
            vocab,
        })
    }

    fn normalize_v1_ipa(s: &str) -> String {
        s.replace('ɾ', "T").replace('ʔ', "t")
    }

    fn extract_entry(val: &Value) -> Option<String> {
        match val {
            Value::String(s) => Some(Self::normalize_v1_ipa(s)),
            Value::Object(map) => map
                .get("DEFAULT")
                .and_then(|d| d.as_str())
                .map(Self::normalize_v1_ipa),
            _ => None,
        }
    }

    fn insert_json_lexicon(
        path: &Path,
        target: &mut HashMap<String, String>,
        overwrite_exact: bool,
    ) -> Result<(), Box<dyn Error>> {
        let raw: HashMap<String, Value> = serde_json::from_str(&fs::read_to_string(path)?)?;
        // First pass: exact entries
        for (k, v) in &raw {
            if let Some(ipa) = Self::extract_entry(v) {
                if overwrite_exact {
                    target.insert(k.clone(), ipa);
                } else {
                    target.entry(k.clone()).or_insert(ipa);
                }
            }
        }
        // Second pass: grow_dictionary lower/capitalized fallbacks
        for (k, v) in raw {
            if k.len() < 2 {
                continue;
            }
            if let Some(ipa) = Self::extract_entry(&v) {
                let lower = k.to_lowercase();
                if k == lower {
                    let mut chars = k.chars();
                    if let Some(first) = chars.next() {
                        let cap = format!("{}{}", first.to_uppercase(), chars.as_str());
                        if cap != k {
                            target.entry(cap).or_insert_with(|| ipa.clone());
                        }
                    }
                } else {
                    target.entry(lower).or_insert_with(|| ipa.clone());
                }
            }
        }
        Ok(())
    }

    fn load_cmudict(path: &Path) -> Result<HashMap<String, String>, Box<dyn Error>> {
        let content = fs::read_to_string(path)?;
        let mut map = HashMap::new();
        for line in content.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(word) = parts.next() else {
                continue;
            };
            if word.contains('(') {
                continue;
            }
            let phones: Vec<&str> = parts.collect();
            let ipa = Self::arpabet_to_kokoro(&phones);
            if !ipa.is_empty() {
                map.insert(word.to_lowercase(), ipa);
            }
        }
        Ok(map)
    }

    fn arpabet_to_kokoro(phones: &[&str]) -> String {
        let mut out = String::new();
        for &p in phones {
            let (base, stress) = if let Some(last) = p.chars().last() {
                if last.is_ascii_digit() {
                    (&p[..p.len() - 1], Some(last))
                } else {
                    (p, None)
                }
            } else {
                (p, None)
            };

            if stress == Some('1') {
                out.push('ˈ');
            } else if stress == Some('2') {
                out.push('ˌ');
            }

            let mapped = match (base, stress) {
                ("AA", _) => "ɑ",
                ("AE", _) => "æ",
                ("AH", Some('0')) => "ə",
                ("AH", _) => "ʌ",
                ("AO", _) => "ɔ",
                ("AW", _) => "W",
                ("AY", _) => "I",
                ("B", _) => "b",
                ("CH", _) => "ʧ",
                ("D", _) => "d",
                ("DH", _) => "ð",
                ("EH", _) => "ɛ",
                ("ER", Some('0')) => "əɹ",
                ("ER", _) => "ɜɹ",
                ("EY", _) => "A",
                ("F", _) => "f",
                ("G", _) => "ɡ",
                ("HH", _) => "h",
                ("IH", _) => "ɪ",
                ("IY", _) => "i",
                ("JH", _) => "ʤ",
                ("K", _) => "k",
                ("L", _) => "l",
                ("M", _) => "m",
                ("N", _) => "n",
                ("NG", _) => "ŋ",
                ("OW", _) => "O",
                ("OY", _) => "Y",
                ("P", _) => "p",
                ("R", _) => "ɹ",
                ("S", _) => "s",
                ("SH", _) => "ʃ",
                ("T", _) => "t",
                ("TH", _) => "θ",
                ("UH", _) => "ʊ",
                ("UW", _) => "u",
                ("V", _) => "v",
                ("W", _) => "w",
                ("Y", _) => "j",
                ("Z", _) => "z",
                ("ZH", _) => "ʒ",
                _ => "",
            };
            out.push_str(mapped);
        }
        out
    }

    fn is_vowel_char(c: char) -> bool {
        matches!(
            c,
            'A' | 'I'
                | 'O'
                | 'Q'
                | 'W'
                | 'Y'
                | 'a'
                | 'i'
                | 'u'
                | 'æ'
                | 'ɑ'
                | 'ɒ'
                | 'ɔ'
                | 'ə'
                | 'ɛ'
                | 'ɜ'
                | 'ɪ'
                | 'ʊ'
                | 'ʌ'
                | 'ᵻ'
                | 'ɐ'
        )
    }

    fn starts_with_vowel_sound(ipa: &str) -> bool {
        ipa.chars()
            .find(|&c| c != 'ˈ' && c != 'ˌ')
            .is_some_and(Self::is_vowel_char)
    }

    fn demote_primary_stress(ipa: &str) -> String {
        ipa.replace('ˌ', "").replace('ˈ', "ˌ")
    }

    fn lookup_direct(&self, word: &str) -> Option<String> {
        let lower = word.to_lowercase();
        self.golds
            .get(word)
            .or_else(|| self.golds.get(&lower))
            .or_else(|| self.silvers.get(word))
            .or_else(|| self.silvers.get(&lower))
            .cloned()
    }

    fn s_suffix(stem_ipa: &str) -> String {
        let Some(last) = stem_ipa.chars().last() else {
            return String::new();
        };
        if "ptkfθ".contains(last) {
            format!("{stem_ipa}s")
        } else if "szʃʒʧʤ".contains(last) {
            format!("{stem_ipa}ᵻz")
        } else {
            format!("{stem_ipa}z")
        }
    }

    fn ed_suffix(stem_ipa: &str) -> String {
        let chars: Vec<char> = stem_ipa.chars().collect();
        let Some(&last) = chars.last() else {
            return String::new();
        };
        if "pkfθʃsʧ".contains(last) {
            format!("{stem_ipa}t")
        } else if last == 'd' {
            format!("{stem_ipa}ᵻd")
        } else if last != 't' {
            format!("{stem_ipa}d")
        } else if chars.len() >= 2 && "AIOWYiuæɑəɛɪɹʊʌ".contains(chars[chars.len() - 2]) {
            let prefix: String = chars[..chars.len() - 1].iter().collect();
            format!("{prefix}Tᵻd")
        } else {
            format!("{stem_ipa}ᵻd")
        }
    }

    fn ing_suffix(stem_ipa: &str) -> String {
        let chars: Vec<char> = stem_ipa.chars().collect();
        let Some(&last) = chars.last() else {
            return String::new();
        };
        if chars.len() >= 2 && last == 't' && "AIOWYiuæɑəɛɪɹʊʌ".contains(chars[chars.len() - 2])
        {
            let prefix: String = chars[..chars.len() - 1].iter().collect();
            format!("{prefix}Tɪŋ")
        } else {
            format!("{stem_ipa}ɪŋ")
        }
    }

    fn phonemize_word(&self, word: &str, next_vowel: Option<bool>) -> String {
        let lower = word.to_lowercase();
        match lower.as_str() {
            "a" => return "ɐ".to_string(),
            "an" => return "ɐn".to_string(),
            "i" => return "ˌI".to_string(),
            "the" => {
                return if next_vowel == Some(true) {
                    "ði".to_string()
                } else {
                    "ðə".to_string()
                };
            }
            "to" => {
                return match next_vowel {
                    Some(true) => "tʊ".to_string(),
                    Some(false) => "tə".to_string(),
                    None => "tˈu".to_string(),
                };
            }
            "in" => return "ɪn".to_string(),
            "of" => return "ʌv".to_string(),
            _ => {}
        }

        if let Some(ipa) = self.lookup_direct(word) {
            return ipa;
        }

        // CMUdict BSD-2-Clause check for exact words/proper nouns (e.g. Elias, Hannah, Josiah, Silas, Agnes)
        if let Some(ipa) = self.cmu.get(&lower) {
            return ipa.clone();
        }

        // Possessive / plural -s
        if lower.len() >= 3 && lower.ends_with("'s") {
            let stem = &word[..word.len() - 2];
            let stem_ipa = self.phonemize_word(stem, None);
            if !stem_ipa.is_empty() {
                return Self::s_suffix(&stem_ipa);
            }
        }
        if lower.len() >= 3 && lower.ends_with('s') && !lower.ends_with("ss") {
            if let Some(stem_ipa) = self.lookup_direct(&lower[..lower.len() - 1]) {
                return Self::s_suffix(&stem_ipa);
            }
            if lower.len() > 4 && lower.ends_with("ies") {
                let y_stem = format!("{}y", &lower[..lower.len() - 3]);
                if let Some(stem_ipa) = self.lookup_direct(&y_stem) {
                    return Self::s_suffix(&stem_ipa);
                }
            }
            if lower.len() > 4 && lower.ends_with("es") {
                if let Some(stem_ipa) = self.lookup_direct(&lower[..lower.len() - 2]) {
                    return Self::s_suffix(&stem_ipa);
                }
            }
        }

        // Past tense -ed / -d
        if lower.len() >= 4 && lower.ends_with('d') && !lower.ends_with("dd") {
            if let Some(stem_ipa) = self.lookup_direct(&lower[..lower.len() - 1]) {
                return Self::ed_suffix(&stem_ipa);
            }
            if lower.len() > 4 && lower.ends_with("ed") && !lower.ends_with("eed") {
                if let Some(stem_ipa) = self.lookup_direct(&lower[..lower.len() - 2]) {
                    return Self::ed_suffix(&stem_ipa);
                }
            }
        }

        // Progressive -ing
        if lower.len() >= 5 && lower.ends_with("ing") {
            let base = &lower[..lower.len() - 3];
            if let Some(stem_ipa) = self.lookup_direct(base) {
                return Self::ing_suffix(&stem_ipa);
            }
            let e_stem = format!("{base}e");
            if let Some(stem_ipa) = self.lookup_direct(&e_stem) {
                return Self::ing_suffix(&stem_ipa);
            }
        }

        // Compound split for two-morpheme place names (Coldspring, Fellgate, Thornhaven, Dryreach, etc.)
        if lower.len() >= 6 {
            for split_idx in 3..=(lower.len() - 3) {
                if !lower.is_char_boundary(split_idx) {
                    continue;
                }
                let left = &lower[..split_idx];
                let right = &lower[split_idx..];
                if let (Some(l_ipa), Some(r_ipa)) = (self.golds.get(left), self.golds.get(right)) {
                    return format!("{}{}", l_ipa, Self::demote_primary_stress(r_ipa));
                }
            }
        }

        // Deterministic rule-based grapheme fallback
        Self::rule_fallback(&lower)
    }

    fn rule_fallback(word: &str) -> String {
        let mut out = String::from("ˈ");
        for c in word.chars() {
            let p = match c {
                'a' => "æ",
                'b' => "b",
                'c' => "k",
                'd' => "d",
                'e' => "ɛ",
                'f' => "f",
                'g' => "ɡ",
                'h' => "h",
                'i' => "ɪ",
                'j' => "ʤ",
                'k' => "k",
                'l' => "l",
                'm' => "m",
                'n' => "n",
                'o' => "ɑ",
                'p' => "p",
                'q' => "k",
                'r' => "ɹ",
                's' => "s",
                't' => "t",
                'u' => "ʌ",
                'v' => "v",
                'w' => "w",
                'x' => "ks",
                'y' => "j",
                'z' => "z",
                _ => "",
            };
            out.push_str(p);
        }
        out
    }

    fn number_to_words(n: u64) -> String {
        const ONES: [&str; 20] = [
            "zero",
            "one",
            "two",
            "three",
            "four",
            "five",
            "six",
            "seven",
            "eight",
            "nine",
            "ten",
            "eleven",
            "twelve",
            "thirteen",
            "fourteen",
            "fifteen",
            "sixteen",
            "seventeen",
            "eighteen",
            "nineteen",
        ];
        const TENS: [&str; 10] = [
            "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
        ];
        if n < 20 {
            ONES[n as usize].to_string()
        } else if n < 100 {
            let t = TENS[(n / 10) as usize];
            let rem = n % 10;
            if rem == 0 {
                t.to_string()
            } else {
                format!("{t} {}", ONES[rem as usize])
            }
        } else if n < 1000 {
            let h = ONES[(n / 100) as usize];
            let rem = n % 100;
            if rem == 0 {
                format!("{h} hundred")
            } else {
                format!("{h} hundred {}", Self::number_to_words(rem))
            }
        } else {
            let th = Self::number_to_words(n / 1000);
            let rem = n % 1000;
            if rem == 0 {
                format!("{th} thousand")
            } else {
                format!("{th} thousand {}", Self::number_to_words(rem))
            }
        }
    }

    fn preprocess_text(text: &str) -> String {
        let mut expanded = String::new();
        for raw_tok in text.split_whitespace() {
            if !expanded.is_empty() {
                expanded.push(' ');
            }
            let leading: String = raw_tok
                .chars()
                .take_while(|c| !c.is_alphanumeric())
                .collect();
            let trailing: String = raw_tok
                .chars()
                .rev()
                .take_while(|c| !c.is_alphanumeric())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let core_end = raw_tok.len().saturating_sub(trailing.len());
            let core = if leading.len() <= core_end {
                &raw_tok[leading.len()..core_end]
            } else {
                ""
            };

            expanded.push_str(&leading);
            if !core.is_empty() && core.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(num) = core.parse::<u64>() {
                    expanded.push_str(&Self::number_to_words(num));
                } else {
                    expanded.push_str(core);
                }
            } else {
                match core.to_lowercase().as_str() {
                    "lbs" => expanded.push_str("pounds"),
                    "lb" => expanded.push_str("pound"),
                    "hrs" => expanded.push_str("hours"),
                    "hr" => expanded.push_str("hour"),
                    _ => expanded.push_str(core),
                }
            }
            expanded.push_str(&trailing);
        }
        expanded
    }

    fn phonemize(&self, text: &str) -> String {
        #[derive(Clone)]
        enum Piece {
            Word(String),
            Punct(String),
        }

        let normalized = Self::preprocess_text(text).replace(['—', '–', '-'], " ");
        let mut pieces = Vec::new();
        for raw in normalized.split_whitespace() {
            let mut cur_word = String::new();
            for ch in raw.chars() {
                if ch.is_alphabetic() || ch == '\'' || ch == '’' {
                    cur_word.push(if ch == '’' { '\'' } else { ch });
                } else if ";:,.!?…\"“”()".contains(ch) {
                    if !cur_word.is_empty() {
                        pieces.push(Piece::Word(std::mem::take(&mut cur_word)));
                    }
                    pieces.push(Piece::Punct(ch.to_string()));
                }
            }
            if !cur_word.is_empty() {
                pieces.push(Piece::Word(cur_word));
            }
        }

        // Right-to-left pass so context-sensitive words ("the", "to") know whether next phoneme is a vowel
        let mut resolved = vec![String::new(); pieces.len()];
        let mut future_vowel: Option<bool> = None;
        for idx in (0..pieces.len()).rev() {
            match &pieces[idx] {
                Piece::Punct(p) => {
                    resolved[idx] = p.clone();
                    future_vowel = None;
                }
                Piece::Word(w) => {
                    let ipa = self.phonemize_word(w, future_vowel);
                    future_vowel = Some(Self::starts_with_vowel_sound(&ipa));
                    resolved[idx] = ipa;
                }
            }
        }

        let mut out = String::new();
        for (idx, piece) in pieces.iter().enumerate() {
            match piece {
                Piece::Word(_) => {
                    if !out.is_empty() && !out.ends_with(' ') {
                        out.push(' ');
                    }
                    out.push_str(&resolved[idx]);
                }
                Piece::Punct(_) => {
                    out.push_str(&resolved[idx]);
                }
            }
        }

        out.chars().filter(|c| self.vocab.contains_key(c)).collect()
    }

    fn tokenize(&self, phonemes: &str) -> Vec<i64> {
        phonemes
            .chars()
            .filter_map(|c| self.vocab.get(&c).copied())
            .collect()
    }
}

/// Parses `voices-v1.0.bin` (uncompressed ZIP `.npz` of `.npy` float32 arrays shaped `[510, 1, 256]`).
fn load_voice_pack(voices_bin_path: &Path, voice_name: &str) -> Result<Vec<f32>, Box<dyn Error>> {
    let bytes = fs::read(voices_bin_path)?;
    let target_entry = format!("{voice_name}.npy");
    let needle = target_entry.as_bytes();

    for pos in 30..bytes.len().saturating_sub(needle.len()) {
        if &bytes[pos..pos + needle.len()] == needle && &bytes[pos - 30..pos - 26] == b"PK\x03\x04"
        {
            let header_start = pos - 30;
            let compression =
                u16::from_le_bytes([bytes[header_start + 8], bytes[header_start + 9]]);
            let name_len =
                u16::from_le_bytes([bytes[header_start + 26], bytes[header_start + 27]]) as usize;
            let extra_len =
                u16::from_le_bytes([bytes[header_start + 28], bytes[header_start + 29]]) as usize;
            if name_len != needle.len() {
                continue;
            }
            if compression != 0 {
                return Err(format!("Expected Stored ZIP entry for {target_entry}").into());
            }
            let data_start = pos + name_len + extra_len;
            if data_start + 10 > bytes.len() || &bytes[data_start..data_start + 6] != b"\x93NUMPY" {
                return Err("Invalid .npy magic header in voices-v1.0.bin".into());
            }
            let header_len =
                u16::from_le_bytes([bytes[data_start + 8], bytes[data_start + 9]]) as usize;
            let floats_start = data_start + 10 + header_len;
            let floats_bytes = 510 * 256 * 4;
            if floats_start + floats_bytes > bytes.len() {
                return Err("Truncated .npy payload in voices-v1.0.bin".into());
            }
            let raw_floats = &bytes[floats_start..floats_start + floats_bytes];
            let floats: Vec<f32> = raw_floats
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&b| f32::from_le_bytes(b))
                .collect();
            return Ok(floats);
        }
    }

    Err(format!("Voice {target_entry} not found in voices-v1.0.bin").into())
}

/// Trims leading and trailing silence matching `kokoro_onnx.trim` (`top_db = 60`, `frame_length = 2048`, `hop_length = 512`).
fn trim_silence(samples: &[f32]) -> Vec<f32> {
    const FRAME_LEN: usize = 2048;
    const HOP_LEN: usize = 512;
    if samples.len() < FRAME_LEN {
        return samples.to_vec();
    }
    let n_frames = 1 + (samples.len() - FRAME_LEN) / HOP_LEN;
    let mut rms_vals = Vec::with_capacity(n_frames);
    let mut max_rms = 0.0f32;
    for i in 0..n_frames {
        let start = i * HOP_LEN;
        let frame = &samples[start..start + FRAME_LEN];
        let mean_sq = frame.iter().map(|&x| x * x).sum::<f32>() / (FRAME_LEN as f32);
        let rms = mean_sq.sqrt();
        if rms > max_rms {
            max_rms = rms;
        }
        rms_vals.push(rms);
    }
    if max_rms <= 1e-8 {
        return samples.to_vec();
    }
    // -60 dB relative to peak RMS = 10^(-60/20) = 0.001
    let threshold = max_rms * 0.001;
    let first = rms_vals.iter().position(|&r| r > threshold);
    let last = rms_vals.iter().rposition(|&r| r > threshold);
    match (first, last) {
        (Some(f), Some(l)) if f <= l => {
            let start_sample = (f * HOP_LEN).min(samples.len());
            let end_sample = ((l + 1) * HOP_LEN).min(samples.len());
            samples[start_sample..end_sample].to_vec()
        }
        _ => samples.to_vec(),
    }
}

fn split_phoneme_batches(phonemes: &str) -> Vec<String> {
    let mut batches = Vec::new();
    let mut current = String::new();
    let mut piece = String::new();

    for ch in phonemes.chars() {
        piece.push(ch);
        if ".,!?;:".contains(ch) {
            let trimmed = piece.trim();
            if !trimmed.is_empty() {
                if current.chars().count() + trimmed.chars().count() + 1 >= MAX_PHONEME_LEN
                    && !current.is_empty()
                {
                    batches.push(current.trim().to_string());
                    current.clear();
                }
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(trimmed);
            }
            piece.clear();
        }
    }
    let tail = piece.trim();
    if !tail.is_empty() {
        if current.chars().count() + tail.chars().count() + 1 >= MAX_PHONEME_LEN
            && !current.is_empty()
        {
            batches.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(tail);
    }
    if !current.trim().is_empty() {
        batches.push(current.trim().to_string());
    }
    batches
}

fn synthesize_line(
    session: &mut Session,
    lexicon: &MisakiLexicon,
    voice_pack: &[f32],
    text: &str,
    speed: f32,
) -> Result<(Vec<f32>, String), Box<dyn Error>> {
    let full_phonemes = lexicon.phonemize(text);
    let batches = split_phoneme_batches(&full_phonemes);
    let mut full_audio = Vec::new();

    for batch in batches {
        let mut tokens = lexicon.tokenize(&batch);
        if tokens.is_empty() {
            continue;
        }
        if tokens.len() > MAX_PHONEME_LEN {
            tokens.truncate(MAX_PHONEME_LEN);
        }
        let style_idx = tokens.len().min(509);
        let style_slice = voice_pack[style_idx * 256..(style_idx + 1) * 256].to_vec();

        let mut padded_tokens = Vec::with_capacity(tokens.len() + 2);
        padded_tokens.push(0i64);
        padded_tokens.extend_from_slice(&tokens);
        padded_tokens.push(0i64);

        let tokens_tensor = Tensor::from_array(([1usize, padded_tokens.len()], padded_tokens))?;
        let style_tensor = Tensor::from_array(([1usize, 256usize], style_slice))?;
        let speed_tensor = Tensor::from_array(([1usize], vec![speed]))?;

        let outputs = session.run(ort::inputs![
            "tokens" => tokens_tensor,
            "style" => style_tensor,
            "speed" => speed_tensor,
        ])?;
        let (_shape, raw_audio) = outputs["audio"].try_extract_tensor::<f32>()?;
        let trimmed = trim_silence(raw_audio);
        full_audio.extend_from_slice(&trimmed);
    }

    Ok((full_audio, full_phonemes))
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<(), Box<dyn Error>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let pcm = (clamped * 32767.0) as i16;
        writer.write_sample(pcm)?;
    }
    writer.finalize()?;
    Ok(())
}

fn play_samples_rodio(samples: &[f32]) -> Result<(), Box<dyn Error>> {
    let mut handle = rodio::DeviceSinkBuilder::open_default_sink()?;
    handle.log_on_drop(false);
    let player = rodio::Player::connect_new(handle.mixer());
    let source = rodio::buffer::SamplesBuffer::new(
        NonZero::new(1).unwrap(),
        NonZero::new(SAMPLE_RATE).unwrap(),
        samples.to_vec(),
    );
    player.append(source);
    player.sleep_until_end();
    Ok(())
}

fn default_cache_dir() -> PathBuf {
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache").join("etv-s3-voice")
}

fn ensure_cache_assets(cache_dir: &Path) -> Result<u64, Box<dyn Error>> {
    fs::create_dir_all(cache_dir)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;
    let mut total_bytes = 0u64;

    for (filename, url) in CACHE_ASSETS {
        let dest = cache_dir.join(filename);
        if !dest.exists() || fs::metadata(&dest)?.len() == 0 {
            println!("[S3] Downloading {filename} from {url} ...");
            let resp = client.get(url).send()?.error_for_status()?;
            let bytes = resp.bytes()?;
            fs::write(&dest, &bytes)?;
        }
        total_bytes += fs::metadata(&dest)?.len();
    }
    Ok(total_bytes)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let mut out_dir: Option<PathBuf> = None;
    let mut play_audio = false;
    let mut single_text: Option<String> = None;
    let mut single_out: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" if i + 1 < args.len() => {
                out_dir = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--play" => {
                play_audio = true;
                i += 1;
            }
            "--text" if i + 1 < args.len() => {
                single_text = Some(args[i + 1].clone());
                i += 2;
            }
            "--out" if i + 1 < args.len() => {
                single_out = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let cache_dir = default_cache_dir();
    let cache_bytes = ensure_cache_assets(&cache_dir)?;
    println!(
        "[S3] Cache directory: {} | total model+lexicon size: {} bytes ({:.2} MB)",
        cache_dir.display(),
        cache_bytes,
        cache_bytes as f64 / (1024.0 * 1024.0)
    );

    let cold_start = Instant::now();
    let env = ort::init().build()?;
    let mut session =
        Session::builder(&env)?.commit_from_file(cache_dir.join("kokoro-v1.0.onnx"))?;
    let lexicon = MisakiLexicon::load(&cache_dir)?;
    let voice_pack = load_voice_pack(&cache_dir.join("voices-v1.0.bin"), VOICE_NAME)?;
    let init_elapsed = cold_start.elapsed();

    if let Some(text) = single_text {
        let (samples, phonemes) =
            synthesize_line(&mut session, &lexicon, &voice_pack, &text, DEFAULT_SPEED)?;
        let first_audio_elapsed = cold_start.elapsed();
        println!(
            "[S3] Cold-start to first audio: {:.3}s (init: {:.3}s) | phonemes: {} | samples: {} ({:.2}s)",
            first_audio_elapsed.as_secs_f64(),
            init_elapsed.as_secs_f64(),
            phonemes,
            samples.len(),
            samples.len() as f64 / SAMPLE_RATE as f64
        );
        if let Some(out_path) = single_out {
            write_wav(&out_path, &samples)?;
        }
        if play_audio {
            play_samples_rodio(&samples)?;
        }
        return Ok(());
    }

    let target_dir = out_dir.unwrap_or_else(|| env::temp_dir().join("etv-s3-rust-wavs"));
    fs::create_dir_all(&target_dir)?;

    for (idx, (id, line)) in GAME_LINES.iter().enumerate() {
        let t0 = Instant::now();
        let (samples, phonemes) =
            synthesize_line(&mut session, &lexicon, &voice_pack, line, DEFAULT_SPEED)?;
        let synth_elapsed = t0.elapsed();
        if idx == 0 {
            let first_audio_elapsed = cold_start.elapsed();
            println!(
                "[S3] Cold-start time to first audio: {:.3}s (model+lexicon load: {:.3}s, line 01 synth: {:.3}s)",
                first_audio_elapsed.as_secs_f64(),
                init_elapsed.as_secs_f64(),
                synth_elapsed.as_secs_f64()
            );
            if play_audio {
                println!("[S3] Playing line 01 via rodio DeviceSinkBuilder...");
                play_samples_rodio(&samples)?;
            }
        }
        let wav_path = target_dir.join(format!("rust-{id}.wav"));
        write_wav(&wav_path, &samples)?;
        println!(
            "[S3][{id}] {:.3}s -> {:.2}s audio | {} | phonemes: {}",
            synth_elapsed.as_secs_f64(),
            samples.len() as f64 / SAMPLE_RATE as f64,
            wav_path.display(),
            phonemes
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_numbers_and_units() {
        assert_eq!(
            MisakiLexicon::preprocess_text("It costs 3 days and 12 lbs of food."),
            "It costs three days and twelve pounds of food."
        );
        assert_eq!(MisakiLexicon::number_to_words(42), "forty two");
        assert_eq!(MisakiLexicon::number_to_words(105), "one hundred five");
    }

    #[test]
    fn silence_trim_removes_leading_and_trailing_zeros() {
        let mut buf = vec![0.0f32; 4096];
        buf.extend(vec![0.5f32; 4096]);
        buf.extend(vec![0.0f32; 4096]);
        let trimmed = trim_silence(&buf);
        assert!(trimmed.len() < buf.len());
        assert!(trimmed.len() >= 4096);
    }
}
