//! What the filter box matches.
//!
//! Typed without accents, as most people type: "yame" has to find Yamê and
//! "angele" Angèle, and a filter that needed the accent found nothing on
//! half of a French library. Case is folded the same way, and the words of
//! the needle may come in any order and land in any field -- "kendrick
//! good" finds the album by its artist and its title together. Symbols
//! standing in for letters are read as the letter: "asap" finds A$AP Mob,
//! "kesha" Ke$ha -- no fuzzy matcher does that, since `$` is not `s` to
//! one, and it is what someone typing the name means.

/// Lowercase, accents taken off.
///
/// A table rather than a normalisation crate: the letters that turn up in
/// artist and track names are the Latin ones, and a dependency to unfold
/// the rest of Unicode buys nothing here.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'ç' | 'ć' | 'č' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' => out.push('i'),
            'ñ' | 'ń' | 'ň' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'š' | 'ś' | 'ş' => out.push('s'),
            'ž' | 'ź' | 'ż' => out.push('z'),
            'ł' => out.push('l'),
            'đ' | 'ď' => out.push('d'),
            'ť' => out.push('t'),
            'ř' => out.push('r'),
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            // Symbols that stand for a letter in a name.
            '$' => out.push('s'),
            '€' => out.push('e'),
            '£' => out.push('l'),
            '¥' => out.push('y'),
            '@' => out.push('a'),
            _ => out.push(c),
        }
    }
    out
}

/// Whether `needle` finds something in `fields`.
pub fn matches(needle: &str, fields: &[&str]) -> bool {
    rank(needle, fields).is_some()
}

/// How well `needle` finds something in `fields`: `Some(0)` when every
/// word of it is somewhere in them, `Some(1)` when every word is only a
/// subsequence -- "kdl" for Kendrick Lamar -- and `None` when a word is
/// neither. Lower is better, so a list sorted by it keeps the real hits
/// above the ones the subsequence lets through.
///
/// Folded as [`fold`] folds; the words may come in any order and land in
/// different fields. An empty needle matches everything at the best rank,
/// which is the filter box before anything is typed.
pub fn rank(needle: &str, fields: &[&str]) -> Option<u8> {
    let needle = fold(needle);
    let mut words = needle.split_whitespace().peekable();
    if words.peek().is_none() {
        return Some(0);
    }
    let hay = fields.iter().map(|f| fold(f)).collect::<Vec<_>>().join(" ");
    let mut worst = 0;
    for w in words {
        if hay.contains(w) {
            continue;
        }
        if is_subsequence(w, &hay) {
            worst = 1;
        } else {
            return None;
        }
    }
    Some(worst)
}

/// Whether the characters of `word` appear in `hay` in order, with anything
/// between them.
fn is_subsequence(word: &str, hay: &str) -> bool {
    let mut wanted = word.chars();
    let mut next = wanted.next();
    for c in hay.chars() {
        if next == Some(c) {
            next = wanted.next();
        }
    }
    next.is_none()
}

/// Keep, in order of rank then of `items`, those an item's fields match.
///
/// One sort for the three filter boxes, so a subsequence hit sits below a
/// real one everywhere.
pub fn ranked<'a, T>(
    items: impl Iterator<Item = (usize, &'a T)>,
    needle: &str,
    fields: impl Fn(&T) -> Vec<&str>,
) -> Vec<(usize, &'a T)> {
    let mut kept: Vec<(u8, usize, &T)> = items
        .filter_map(|(i, item)| rank(needle, &fields(item)).map(|r| (r, i, item)))
        .collect();
    kept.sort_by_key(|(r, i, _)| (*r, *i));
    kept.into_iter().map(|(_, i, item)| (i, item)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_case_do_not_matter_either_way() {
        // The reported case: typing "yame" found nothing.
        assert!(matches("yame", &["Yamê"]));
        assert!(matches("Yamê", &["yame"]), "and the other way about");
        assert!(matches("ANGELE", &["Angèle", "Bruxelles je t'aime"]));
        assert!(matches("strasse", &["Straße"]));
        assert!(!matches("xyz", &["Yamê"]));
    }

    #[test]
    fn a_symbol_standing_for_a_letter_is_that_letter() {
        assert!(matches("asap", &["A$AP Mob"]));
        assert!(matches("kesha", &["Ke$ha"]));
        assert!(
            matches("a$ap", &["A$AP Mob"]),
            "typed as written still finds it"
        );
        assert_eq!(
            rank("aap", &["A$AP Mob"]),
            Some(1),
            "without it, only a subsequence"
        );
    }

    #[test]
    fn the_words_land_anywhere_in_any_order() {
        let fields = ["good kid, m.A.A.d city", "Kendrick Lamar", ""];
        assert!(matches("kendrick good", &fields), "artist then title");
        assert!(matches("good kendrick", &fields), "title then artist");
        assert!(matches("m.a.a.d", &fields), "punctuation is kept as typed");
        assert!(!matches("kendrick bad", &fields), "every word has to land");
    }

    #[test]
    fn a_subsequence_is_found_but_ranked_below_a_real_hit() {
        assert_eq!(
            rank("kdl", &["Kendrick Lamar"]),
            Some(1),
            "k..d..l, in order"
        );
        assert_eq!(rank("kendrick", &["Kendrick Lamar"]), Some(0));
        assert_eq!(
            rank("ldk", &["Kendrick Lamar"]),
            None,
            "out of order is not it"
        );
        // Every word decides on its own; the worst of them is the rank.
        assert_eq!(rank("kendrick lmr", &["Kendrick Lamar"]), Some(1));

        let names = ["Lana Del Rey", "Kendrick Lamar", "Kid Cudi"];
        let kept = ranked(names.iter().enumerate(), "kdl", |n| vec![n]);
        let order: Vec<&str> = kept.iter().map(|(_, n)| **n).collect();
        assert_eq!(
            order,
            ["Kendrick Lamar"],
            "Kid Cudi has a k and a d but no l: gone"
        );

        // And a real hit comes first however far down the list it sits.
        let names = ["Kadl", "Kendrick Lamar", "kdl"];
        let kept = ranked(names.iter().enumerate(), "kdl", |n| vec![n]);
        let order: Vec<&str> = kept.iter().map(|(_, n)| **n).collect();
        assert_eq!(order, ["kdl", "Kadl", "Kendrick Lamar"]);
    }

    #[test]
    fn nothing_typed_matches_everything() {
        assert!(matches("", &["anything"]));
        assert!(matches("   ", &["anything"]));
    }
}
