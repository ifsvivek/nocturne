//! Choosing between search results: a score per result, an automatic pick only when one result is clearly the
//! best, otherwise the closest few go to the user ("Choose the right match").

use nocturne_local::{Candidate, fold};

/// One search result from any provider.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub year: Option<u16>,
    pub overview: String,
}

impl Hit {
    pub fn candidate(&self, provider: &str) -> Candidate {
        Candidate {
            provider: provider.into(),
            id: self.id.clone(),
            title: self.title.clone(),
            year: self.year,
            overview: self.overview.chars().take(240).collect(),
        }
    }
}

/// An automatic match needs at least this score ...
pub const ACCEPT: f32 = 0.85;
/// ... and the runner-up must trail by at least this much.
pub const MARGIN: f32 = 0.15;
/// Results below this are not worth showing.
const PLAUSIBLE: f32 = 0.4;
const SHOWN: usize = 5;

#[derive(Debug, PartialEq)]
pub enum Decision {
    Match(Hit),
    Ask(Vec<Hit>),
    Nothing,
}

fn words(s: &str) -> Vec<String> {
    let f = fold(s);
    let mut w: Vec<String> = f
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    // "The Matrix" and "Matrix" are the same title.
    if w.len() > 1 && matches!(w[0].as_str(), "the" | "a" | "an") {
        w.remove(0);
    }
    w
}

/// 0..=1: equal after folding and dropping punctuation and a leading article = 1; otherwise word overlap.
fn title_sim(a: &str, b: &str) -> f32 {
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let common = a.iter().filter(|w| b.contains(w)).count() as f32;
    // Dice coefficient, capped below "equal" so a near title never beats an exact one.
    (2.0 * common / (a.len() + b.len()) as f32).min(0.9)
}

/// Exact title and year = 1.0; exact title, year unknown on a side = 0.85; year one off = 0.9; wrong year = 0.5.
pub fn score(q_title: &str, q_year: Option<u16>, h: &Hit) -> f32 {
    let year = match (q_year, h.year) {
        (Some(a), Some(b)) if a == b => 0.2,
        (Some(a), Some(b)) if a.abs_diff(b) == 1 => 0.1,
        (Some(_), Some(_)) => -0.3,
        _ => 0.05,
    };
    0.8 * title_sim(q_title, &h.title) + year
}

pub fn decide(q_title: &str, q_year: Option<u16>, hits: Vec<Hit>) -> Decision {
    let mut s: Vec<(f32, Hit)> = hits
        .into_iter()
        .map(|h| (score(q_title, q_year, &h), h))
        .filter(|(s, _)| *s >= PLAUSIBLE)
        .collect();
    s.sort_by(|a, b| b.0.total_cmp(&a.0));
    match s.as_slice() {
        [] => Decision::Nothing,
        [(best, _), rest @ ..]
            if *best >= ACCEPT && rest.first().is_none_or(|(n, _)| best - n >= MARGIN) =>
        {
            Decision::Match(s.swap_remove(0).1)
        }
        _ => Decision::Ask(s.into_iter().take(SHOWN).map(|(_, h)| h).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(id: &str, title: &str, year: Option<u16>) -> Hit {
        Hit {
            id: id.into(),
            title: title.into(),
            year,
            overview: String::new(),
        }
    }

    #[test]
    fn one_clear_result_is_taken() {
        let d = decide(
            "The Matrix",
            Some(1999),
            vec![
                h("1", "The Matrix", Some(1999)),
                h("2", "The Matrix Reloaded", Some(2003)),
            ],
        );
        assert_eq!(d, Decision::Match(h("1", "The Matrix", Some(1999))));
        // Article, case, punctuation and diacritics do not matter; a year one off still passes.
        assert!(matches!(
            decide("matrix", Some(2000), vec![h("1", "The Matrix", Some(1999))]),
            Decision::Match(_)
        ));
        assert!(matches!(
            decide("Amélie", None, vec![h("1", "Amelie", Some(2001))]),
            Decision::Match(_)
        ));
    }

    #[test]
    fn same_title_without_a_year_goes_to_the_user() {
        // Two remakes and no year to tell them apart: never guess.
        let d = decide(
            "Solaris",
            None,
            vec![h("1", "Solaris", Some(1972)), h("2", "Solaris", Some(2002))],
        );
        let Decision::Ask(v) = d else { panic!("{d:?}") };
        assert_eq!(v.len(), 2);
        // The year settles it.
        assert!(matches!(
            decide(
                "Solaris",
                Some(1972),
                vec![h("1", "Solaris", Some(1972)), h("2", "Solaris", Some(2002))]
            ),
            Decision::Match(_)
        ));
    }

    #[test]
    fn wrong_year_and_loose_titles_are_not_auto_matched() {
        assert!(matches!(
            decide("Heat", Some(1995), vec![h("1", "Heat", Some(1986))]),
            Decision::Ask(_)
        ));
        assert_eq!(
            decide("Zzzz", None, vec![h("1", "Completely Different", None)]),
            Decision::Nothing
        );
        assert_eq!(decide("x", None, vec![]), Decision::Nothing);
    }

    #[test]
    fn at_most_five_are_offered() {
        let hits = (0..9)
            .map(|i| h(&i.to_string(), "Dune", Some(1980 + i)))
            .collect();
        let Decision::Ask(v) = decide("Dune", None, hits) else {
            panic!()
        };
        assert_eq!(v.len(), 5);
    }
}
