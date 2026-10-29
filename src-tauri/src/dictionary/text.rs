//! Picking the word, and the sentence it is in, out of text read from the screen.

/// Anything longer than this is not a word worth looking up.
const MAX_WORD_CHARS: usize = 48;
/// The most of a sentence kept as the text around a word.
const MAX_CONTEXT_CHARS: usize = 400;

/// The word in `raw` (as read from the screen) without the spaces and punctuation around
/// it. `None` when there is no word: only digits or symbols, or something far too long.
pub fn clean_word(raw: &str) -> Option<String> {
    let edge = |c: char| !c.is_alphanumeric();
    let first = raw.trim_matches(edge).split(char::is_whitespace).next()?.trim_matches(edge);
    let ok = !first.is_empty()
        && first.chars().count() <= MAX_WORD_CHARS
        && first.chars().any(char::is_alphabetic);
    ok.then(|| first.to_string())
}

fn same_letters(a: &[char], b: &[char]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_lowercase().eq(y.to_lowercase()))
}

/// Ends a sentence: a full stop, question or exclamation mark before a space.
fn ends_sentence(chars: &[char], i: usize) -> bool {
    matches!(chars[i], '.' | '!' | '?') && chars.get(i + 1).is_none_or(|c| c.is_whitespace())
}

/// The sentence of `text` that holds `word`, taking the occurrence nearest the character
/// position `near`. Empty when `word` is not in `text`. Long sentences are cut to the part
/// around the word, marked with "…".
pub fn sentence_around(text: &str, near: usize, word: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let target: Vec<char> = word.chars().collect();
    if target.is_empty() || target.len() > chars.len() {
        return String::new();
    }
    let Some(at) = (0..=chars.len() - target.len())
        .filter(|&i| same_letters(&chars[i..i + target.len()], &target))
        .min_by_key(|&i| i.abs_diff(near))
    else {
        return String::new();
    };
    let mut start = at;
    while start > 0 && chars[start - 1] != '\n' && !ends_sentence(&chars, start - 1) {
        start -= 1;
    }
    let mut end = at + target.len();
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
        if ends_sentence(&chars, end - 1) {
            break;
        }
    }
    shorten(&chars[start..end], at - start, target.len())
}

/// Collapses runs of spaces, and keeps at most `MAX_CONTEXT_CHARS` around the word that
/// starts at `at`, cutting between words.
fn shorten(sentence: &[char], at: usize, len: usize) -> String {
    let (mut from, mut to) = (0, sentence.len());
    if to - from > MAX_CONTEXT_CHARS {
        let spare = MAX_CONTEXT_CHARS.saturating_sub(len) / 2;
        from = at.saturating_sub(spare);
        to = (at + len + spare).min(sentence.len());
        while from > 0 && from < at && !sentence[from - 1].is_whitespace() {
            from += 1;
        }
        while to < sentence.len() && to > at + len && !sentence[to].is_whitespace() {
            to -= 1;
        }
    }
    let body: String = sentence[from..to].iter().collect();
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let before = if from > 0 { "…" } else { "" };
    let after = if to < sentence.len() { "…" } else { "" };
    format!("{before}{body}{after}")
}

/// A word read from a picture, and where it is in that picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Placed {
    /// How far the point is outside the word's box (0 inside it).
    fn distance(&self, x: f32, y: f32) -> f32 {
        let dx = (self.x - x).max(x - (self.x + self.w)).max(0.0);
        let dy = (self.y - y).max(y - (self.y + self.h)).max(0.0);
        dx.hypot(dy)
    }
}

/// The word under the point, or else the nearest one no more than `slack` away.
pub fn word_at(words: &[Placed], x: f32, y: f32, slack: f32) -> Option<usize> {
    words
        .iter()
        .enumerate()
        .map(|(i, w)| (i, w.distance(x, y)))
        .filter(|&(_, d)| d <= slack)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Whether any of the boxes in `values` (left, top, width, height, in turn) holds the
/// point, give or take `slack`. No boxes at all means the app did not say, which counts.
pub fn boxes_hold(values: &[f64], x: f64, y: f64, slack: f64) -> bool {
    let boxes = values.as_chunks::<4>().0;
    boxes.is_empty()
        || boxes.iter().any(|b| {
            x >= b[0] - slack
                && x <= b[0] + b[2] + slack
                && y >= b[1] - slack
                && y <= b[1] + b[3] + slack
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_loses_the_spaces_and_punctuation_around_it() {
        assert_eq!(clean_word("serendipity, ").as_deref(), Some("serendipity"));
        assert_eq!(clean_word("\u{201c}Hello!\u{201d}").as_deref(), Some("Hello"));
        assert_eq!(clean_word("don't").as_deref(), Some("don't"));
        assert_eq!(clean_word("well-known.").as_deref(), Some("well-known"));
        assert_eq!(clean_word("two words").as_deref(), Some("two"));
    }

    #[test]
    fn numbers_symbols_and_very_long_runs_are_not_words() {
        for raw in ["", "  ", "1984", "—", "#$%", &"a".repeat(60)] {
            assert_eq!(clean_word(raw), None, "{raw:?}");
        }
        assert_eq!(clean_word("3D").as_deref(), Some("3D"));
    }

    #[test]
    fn the_sentence_holding_the_word_is_kept() {
        let text = "It rained. The ubiquitous pigeons gathered! Then they left.";
        assert_eq!(sentence_around(text, 15, "ubiquitous"), "The ubiquitous pigeons gathered!");
        assert_eq!(sentence_around(text, 0, "rained"), "It rained.");
        assert_eq!(sentence_around(text, 50, "left"), "Then they left.");
    }

    #[test]
    fn the_occurrence_nearest_the_pointer_is_used() {
        let text = "A bank lends money. We sat on the bank of the river.";
        assert_eq!(sentence_around(text, 40, "bank"), "We sat on the bank of the river.");
        assert_eq!(sentence_around(text, 0, "Bank"), "A bank lends money.");
    }

    #[test]
    fn line_breaks_end_a_sentence_and_spaces_are_tidied() {
        let text = "Heading\nSome   text\twith gaps\nFooter";
        assert_eq!(sentence_around(text, 10, "text"), "Some text with gaps");
    }

    #[test]
    fn abbreviations_and_decimals_do_not_end_a_sentence() {
        let text = "It costs 3.50 dollars, e.g.in shops.";
        assert_eq!(sentence_around(text, 14, "dollars"), text);
    }

    #[test]
    fn a_word_that_is_not_there_has_no_sentence() {
        assert_eq!(sentence_around("Nothing here.", 0, "pigeon"), "");
        assert_eq!(sentence_around("", 0, "pigeon"), "");
    }

    #[test]
    fn a_long_sentence_is_cut_around_the_word() {
        let filler = "word ".repeat(200);
        let text = format!("{filler}target {filler}");
        let near = filler.chars().count();
        let short = sentence_around(&text, near, "target");
        assert!(short.contains("target"));
        assert!(short.starts_with('…') && short.ends_with('…'), "{short}");
        assert!(short.chars().count() <= MAX_CONTEXT_CHARS + 2);
        assert!(!short.contains("wor…") && !short.contains("…ord"), "cut between words");
    }

    fn placed(text: &str, x: f32) -> Placed {
        Placed { text: text.into(), x, y: 10.0, w: 40.0, h: 12.0 }
    }

    #[test]
    fn the_word_under_the_point_wins_then_the_nearest_close_one() {
        let words = [placed("one", 0.0), placed("two", 50.0), placed("three", 100.0)];
        assert_eq!(word_at(&words, 60.0, 15.0, 4.0), Some(1));
        assert_eq!(word_at(&words, 42.0, 15.0, 4.0), Some(0), "2 px past 'one', 8 before 'two'");
        assert_eq!(word_at(&words, 47.0, 15.0, 4.0), Some(1), "7 px past 'one', 3 before 'two'");
        assert_eq!(word_at(&words, 300.0, 15.0, 4.0), None, "nothing near");
        assert_eq!(word_at(&[], 0.0, 0.0, 4.0), None);
    }

    #[test]
    fn boxes_hold_the_point_with_some_slack() {
        let boxes = [10.0, 10.0, 50.0, 20.0, 100.0, 10.0, 30.0, 20.0];
        assert!(boxes_hold(&boxes, 20.0, 20.0, 0.0));
        assert!(boxes_hold(&boxes, 110.0, 15.0, 0.0));
        assert!(boxes_hold(&boxes, 62.0, 20.0, 3.0));
        assert!(!boxes_hold(&boxes, 80.0, 20.0, 3.0));
        assert!(boxes_hold(&[], 0.0, 0.0, 0.0), "no boxes: the app did not say");
    }
}
