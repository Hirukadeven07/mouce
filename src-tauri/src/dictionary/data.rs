//! The offline dictionary: Open English WordNet (CC BY 4.0), trimmed by
//! `dictionary/build_dictionary.py` and built into the program, so a word can be looked up
//! with no AI and no network. No file is opened to read it.
//!
//! The data is a list of zlib-compressed blocks of sorted words (see the script for the
//! layout). A lookup unpacks only the one or two blocks it needs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

static DATA: &[u8] = include_bytes!("../../dictionary/oewn.dict");
const MAGIC: &[u8] = b"MOUCEDICT1";

/// One meaning of a word, with an example of it in use when the dictionary has one.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Sense {
    pub meaning: String,
    pub example: Option<String>,
}

/// A word as one part of speech: "run" the verb and "run" the noun are two entries.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub word: String,
    pub part_of_speech: &'static str,
    /// In the International Phonetic Alphabet, when the dictionary has it.
    pub pronunciation: Option<String>,
    pub senses: Vec<Sense>,
}

#[derive(Deserialize, Default)]
struct RawRecord {
    #[serde(default)]
    e: Vec<RawEntry>,
    /// The words this one is a form of ("ran" is a form of "run").
    #[serde(default)]
    f: Vec<String>,
}

#[derive(Deserialize)]
struct RawEntry {
    w: String,
    p: String,
    i: Option<String>,
    s: Vec<Vec<String>>,
}

impl RawEntry {
    fn to_entry(&self) -> Entry {
        Entry {
            word: self.w.clone(),
            part_of_speech: part_of_speech(&self.p),
            pronunciation: self.i.clone(),
            senses: self
                .s
                .iter()
                .filter_map(|s| {
                    let meaning = s.first()?.clone();
                    Some(Sense { meaning, example: s.get(1).cloned() })
                })
                .collect(),
        }
    }
}

fn part_of_speech(code: &str) -> &'static str {
    match code {
        "n" => "noun",
        "v" => "verb",
        "a" => "adjective",
        "r" => "adverb",
        _ => "",
    }
}

struct Block {
    first: String,
    start: usize,
    len: usize,
}

pub struct Dictionary<'a> {
    blocks: Vec<Block>,
    data: &'a [u8],
}

fn read_u32(bytes: &[u8], at: &mut usize) -> Result<usize, String> {
    let raw = bytes.get(*at..*at + 4).ok_or("the dictionary is cut short")?;
    *at += 4;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize)
}

impl<'a> Dictionary<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Dictionary<'a>, String> {
        let rest = bytes.strip_prefix(MAGIC).ok_or("not a mouce dictionary")?;
        let mut at = 0;
        let count = read_u32(rest, &mut at)?;
        let mut blocks = Vec::with_capacity(count);
        for _ in 0..count {
            let len = read_u32(rest, &mut at)?;
            let key = rest.get(at..at + len).ok_or("the dictionary is cut short")?;
            let first = String::from_utf8(key.to_vec()).map_err(|e| e.to_string())?;
            at += len;
            let start = read_u32(rest, &mut at)?;
            let len = read_u32(rest, &mut at)?;
            blocks.push(Block { first, start, len });
        }
        let data = &rest[at..];
        if blocks.iter().any(|b| b.start + b.len > data.len()) {
            return Err("the dictionary is cut short".to_string());
        }
        Ok(Dictionary { blocks, data })
    }

    /// The record stored under `key` (a word in lower case), if there is one.
    fn record(&self, key: &str) -> Option<RawRecord> {
        // The last block starting at or before `key` is the only one that can hold it.
        let i = self.blocks.partition_point(|b| b.first.as_str() <= key).checked_sub(1)?;
        let block = &self.blocks[i];
        let packed = &self.data[block.start..block.start + block.len];
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(packed).ok()?;
        let mut records: HashMap<String, RawRecord> = serde_json::from_slice(&raw).ok()?;
        records.remove(key)
    }

    /// The entries for `word` as written on screen: the word itself, or the word it is a
    /// form of ("geese" finds "goose", "running" finds "run"). Empty when it is unknown.
    pub fn lookup(&self, word: &str) -> Vec<Entry> {
        let key = normalize(word);
        if key.is_empty() {
            return Vec::new();
        }
        let mut found = Found::default();
        let mut keys = vec![key.clone()];
        if let Some(base) = key.strip_suffix("'s") {
            keys.push(base.to_string());
        }
        for key in &keys {
            if let Some(record) = self.record(key) {
                found.add(&record.e, None);
                for base in &record.f {
                    if let Some(r) = self.record(base) {
                        found.add(&r.e, None);
                    }
                }
            }
        }
        // Only guess at endings when the word is not in the dictionary as written: "is"
        // must not turn up "i" by dropping its "s".
        if found.entries.is_empty() {
            for (pos, base) in keys.iter().flat_map(|k| base_forms(k)) {
                if let Some(record) = self.record(&base) {
                    found.add(&record.e, Some(pos));
                }
            }
        }
        found.entries
    }
}

#[derive(Default)]
struct Found {
    entries: Vec<Entry>,
}

impl Found {
    /// Adds the entries not already found, only those of `pos` when it is given.
    fn add(&mut self, raw: &[RawEntry], pos: Option<&str>) {
        for r in raw.iter().filter(|r| pos.is_none_or(|p| r.p == p)) {
            let entry = r.to_entry();
            let known = self
                .entries
                .iter()
                .any(|e| e.word == entry.word && e.part_of_speech == entry.part_of_speech);
            if !known && !entry.senses.is_empty() {
                self.entries.push(entry);
            }
        }
    }
}

/// How a word is stored: lower case, with straight apostrophes.
fn normalize(word: &str) -> String {
    word.trim().replace(['\u{2019}', '\u{2018}'], "'").to_lowercase()
}

/// WordNet's regular endings, as (part of speech, ending, replacement).
const ENDINGS: &[(&str, &str, &str)] = &[
    ("n", "s", ""),
    ("n", "ses", "s"),
    ("n", "xes", "x"),
    ("n", "zes", "z"),
    ("n", "ches", "ch"),
    ("n", "shes", "sh"),
    ("n", "men", "man"),
    ("n", "ies", "y"),
    ("v", "s", ""),
    ("v", "ies", "y"),
    ("v", "es", "e"),
    ("v", "es", ""),
    ("v", "ed", "e"),
    ("v", "ed", ""),
    ("v", "ing", "e"),
    ("v", "ing", ""),
    ("a", "er", ""),
    ("a", "est", ""),
    ("a", "er", "e"),
    ("a", "est", "e"),
];

/// The words `key` could be an ending of, with the part of speech each would be. A doubled
/// last letter is undone too ("stopped" -> "stop", "bigger" -> "big").
fn base_forms(key: &str) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for &(pos, ending, replacement) in ENDINGS {
        let Some(stem) = key.strip_suffix(ending) else { continue };
        if stem.chars().count() < 2 {
            continue;
        }
        let base = format!("{stem}{replacement}");
        if !out.contains(&(pos, base.clone())) {
            out.push((pos, base));
        }
        let mut chars = stem.chars().rev();
        let doubled = matches!((chars.next(), chars.next()), (Some(a), Some(b)) if a == b && !"aeiouls".contains(a));
        if doubled && replacement.is_empty() && ending != "s" {
            let mut undoubled = stem.to_string();
            undoubled.pop();
            if !out.contains(&(pos, undoubled.clone())) {
                out.push((pos, undoubled));
            }
        }
    }
    out
}

/// The dictionary built into mouce. An unreadable one (which the tests rule out) is empty.
pub fn built_in() -> &'static Dictionary<'static> {
    static DICTIONARY: OnceLock<Dictionary<'static>> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        Dictionary::parse(DATA).unwrap_or_else(|e| {
            log::error!("the offline dictionary could not be read: {e}");
            Dictionary { blocks: Vec::new(), data: &[] }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dictionary file in the same layout as `build_dictionary.py` writes, from blocks
    /// of `(key, record JSON)`.
    fn file(blocks: &[&[(&str, &str)]]) -> Vec<u8> {
        let mut index = Vec::new();
        let mut data = Vec::new();
        for block in blocks {
            let body: Vec<String> = block.iter().map(|(k, r)| format!("\"{k}\":{r}")).collect();
            let packed = miniz_oxide::deflate::compress_to_vec_zlib(
                format!("{{{}}}", body.join(",")).as_bytes(),
                6,
            );
            index.push((block[0].0, data.len(), packed.len()));
            data.extend(packed);
        }
        let mut out = MAGIC.to_vec();
        out.extend((index.len() as u32).to_le_bytes());
        for (first, start, len) in index {
            out.extend((first.len() as u32).to_le_bytes());
            out.extend(first.as_bytes());
            out.extend((start as u32).to_le_bytes());
            out.extend((len as u32).to_le_bytes());
        }
        out.extend(data);
        out
    }

    const GOOSE: &str = r#"{"e":[{"w":"goose","p":"n","i":"ɡuːs","s":[["a large waterbird","geese fly south"],["a silly person"]]}]}"#;
    const RUN: &str = r#"{"e":[{"w":"run","p":"n","s":[["a race"]]},{"w":"run","p":"v","s":[["move fast on foot"]]}]}"#;

    fn sample() -> Vec<u8> {
        file(&[
            &[("geese", r#"{"f":["goose"]}"#), ("goose", GOOSE)],
            &[
                ("ran", r#"{"f":["run"]}"#),
                ("run", RUN),
                ("stop", r#"{"e":[{"w":"stop","p":"v","s":[["halt"]]}]}"#),
            ],
        ])
    }

    #[test]
    fn a_word_is_found_with_its_meanings_examples_and_pronunciation() {
        let bytes = sample();
        let d = Dictionary::parse(&bytes).unwrap();
        let entries = d.lookup("Goose");
        assert_eq!(entries.len(), 1);
        let goose = &entries[0];
        assert_eq!((goose.word.as_str(), goose.part_of_speech), ("goose", "noun"));
        assert_eq!(goose.pronunciation.as_deref(), Some("ɡuːs"));
        assert_eq!(goose.senses[0].meaning, "a large waterbird");
        assert_eq!(goose.senses[0].example.as_deref(), Some("geese fly south"));
        assert_eq!(goose.senses[1].example, None);
    }

    #[test]
    fn a_listed_form_leads_to_its_word() {
        let bytes = sample();
        let d = Dictionary::parse(&bytes).unwrap();
        assert_eq!(d.lookup("geese")[0].word, "goose");
        let ran: Vec<_> = d.lookup("ran").iter().map(|e| e.part_of_speech).collect();
        assert_eq!(ran, vec!["noun", "verb"]);
    }

    #[test]
    fn a_regular_ending_finds_the_word_as_that_part_of_speech() {
        let bytes = sample();
        let d = Dictionary::parse(&bytes).unwrap();
        let runs: Vec<_> = d.lookup("runs").iter().map(|e| e.part_of_speech).collect();
        assert_eq!(runs, vec!["noun", "verb"], "runs is a plural noun or a verb");
        let running: Vec<_> = d.lookup("running").iter().map(|e| e.part_of_speech).collect();
        assert_eq!(running, vec!["verb"], "-ing finds only the verb");
        assert_eq!(d.lookup("stopped")[0].word, "stop");
        assert_eq!(d.lookup("goose\u{2019}s")[0].word, "goose");
    }

    #[test]
    fn an_unknown_word_finds_nothing() {
        let bytes = sample();
        let d = Dictionary::parse(&bytes).unwrap();
        assert!(d.lookup("aardvark").is_empty(), "before the first block");
        assert!(d.lookup("zebra").is_empty(), "after the last word");
        assert!(d.lookup("  ").is_empty());
    }

    #[test]
    fn a_damaged_file_is_refused() {
        assert!(Dictionary::parse(b"nope").is_err());
        let mut bytes = sample();
        bytes.truncate(bytes.len() - 10);
        assert!(Dictionary::parse(&bytes).is_err());
    }

    #[test]
    fn endings_are_undone_like_wordnet() {
        let forms = base_forms("boxes");
        assert!(forms.contains(&("n", "box".into())));
        assert!(forms.contains(&("v", "boxe".into())));
        assert!(base_forms("bigger").contains(&("a", "big".into())));
        assert!(base_forms("hopped").contains(&("v", "hop".into())));
        assert!(!base_forms("called").contains(&("v", "cal".into())), "ll stays doubled");
        assert!(base_forms("is").is_empty(), "too short to have an ending");
    }

    #[test]
    fn the_built_in_dictionary_knows_everyday_words() {
        let d = built_in();
        assert!(d.blocks.len() > 100, "the whole dictionary is built in");
        let run = d.lookup("ran");
        assert!(run.iter().any(|e| e.word == "run" && e.part_of_speech == "verb"));
        assert_eq!(d.lookup("geese")[0].word, "goose");
        let serendipity = d.lookup("serendipity");
        assert!(serendipity[0].senses[0].meaning.contains("luck"), "{serendipity:?}");
        assert!(serendipity[0].pronunciation.is_some());
        assert!(d.lookup("ubiquitous")[0].part_of_speech == "adjective");
        assert_eq!(d.lookup("reorganizing")[0].word, "reorganize", "-ing replaced by -e");
        assert!(d.lookup("qwzxv").is_empty());
    }
}
