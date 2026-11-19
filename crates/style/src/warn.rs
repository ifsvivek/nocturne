//! Warnings grouped by kind: a count and the first few examples per kind, so a noisy kind never hides a rare one.
use std::collections::BTreeMap;

/// Ordered by importance: the first kinds are the ones a theme author should look at first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// The essential floor overrode a theme rule.
    Floor,
    /// A size, count or time limit cut something off.
    Limit,
    Syntax,
    UnsupportedSelector,
    UnsupportedAtRule,
    UnsupportedProperty,
    BadValue,
    /// Valid CSS that is planned but not drawn yet.
    Planned,
    /// A `var()` that cannot be resolved.
    Variable,
    /// Drawn through a documented fallback (`backdrop-filter`, `mask-image`): accepted, not a failure.
    Approximated,
    /// Browser-only properties that mean nothing in a desktop app: accepted silently, listed last, never a failure.
    NoEffect,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Floor => "essential controls kept visible",
            Kind::Limit => "limits reached",
            Kind::Syntax => "syntax problems",
            Kind::UnsupportedSelector => "unsupported selectors",
            Kind::UnsupportedAtRule => "unsupported at-rules",
            Kind::UnsupportedProperty => "unsupported properties",
            Kind::BadValue => "values that could not be read",
            Kind::Planned => "planned, not drawn yet",
            Kind::Variable => "unresolved variables",
            Kind::Approximated => "drawn through a fallback",
            Kind::NoEffect => "no effect in a desktop app",
        }
    }

    /// Low-priority kinds are accepted rules, not failures.
    pub fn is_failure(self) -> bool {
        !matches!(self, Kind::NoEffect | Kind::Approximated)
    }
}

/// Examples kept per kind.
pub const EXAMPLES: usize = 5;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub count: usize,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub groups: BTreeMap<Kind, Group>,
}

impl Report {
    pub fn add(&mut self, kind: Kind, msg: impl Into<String>) {
        self.add_n(kind, 1, msg);
    }

    pub fn add_n(&mut self, kind: Kind, n: usize, msg: impl Into<String>) {
        let g = self.groups.entry(kind).or_default();
        g.count += n;
        if g.examples.len() < EXAMPLES {
            let m = msg.into();
            if !g.examples.contains(&m) {
                g.examples.push(m);
            }
        }
    }

    pub fn merge(&mut self, o: &Report) {
        for (k, g) in &o.groups {
            let mine = self.groups.entry(*k).or_default();
            mine.count += g.count;
            for e in &g.examples {
                if mine.examples.len() < EXAMPLES && !mine.examples.contains(e) {
                    mine.examples.push(e.clone());
                }
            }
        }
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.groups.get(&kind).map_or(0, |g| g.count)
    }

    /// Everything that is a failure (not the low-priority group).
    pub fn failures(&self) -> usize {
        self.groups
            .iter()
            .filter(|(k, _)| k.is_failure())
            .map(|(_, g)| g.count)
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// One line per kind, most important first: `3 unsupported properties: a, b, c`.
    pub fn lines(&self) -> Vec<String> {
        self.groups
            .iter()
            .map(|(k, g)| format!("{} {}: {}", g.count, k.label(), g.examples.join("; ")))
            .collect()
    }
}
