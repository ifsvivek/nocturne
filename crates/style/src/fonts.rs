//! `font-family` resolution: a CSS list is matched against the fonts the system has plus fonts the app registered at run time
//! by name (the fetching of font files is the app's business). The first available family wins; generic names map to the
//! app's own faces.
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontChoice {
    /// The app's own interface font.
    #[default]
    Default,
    Serif,
    Monospace,
    /// A family by name (installed or registered).
    Named(Rc<str>),
}

#[derive(Clone, Debug, Default)]
pub struct Fonts {
    /// Lowercase name -> name as the system spells it.
    installed: HashMap<String, Rc<str>>,
    registered: HashMap<String, Rc<str>>,
    generation: u32,
}

impl Fonts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the list of installed families (the app reads it from the text shaper's font database).
    pub fn set_installed<I: IntoIterator<Item = String>>(&mut self, names: I) {
        self.installed = names
            .into_iter()
            .map(|n| (n.to_lowercase(), Rc::from(n)))
            .collect();
        self.generation += 1;
    }

    /// A family the app loaded at run time (for example a font a server theme asked for and the app fetched).
    pub fn register(&mut self, name: &str) {
        self.registered
            .insert(name.trim().to_lowercase(), Rc::from(name.trim()));
        self.generation += 1;
    }

    pub fn unregister(&mut self, name: &str) {
        self.registered.remove(&name.trim().to_lowercase());
        self.generation += 1;
    }

    /// Every family the app can use by name, sorted (for the Studio's font list).
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .registered
            .values()
            .chain(self.installed.values())
            .map(|n| n.to_string())
            .collect();
        v.sort_by_key(|n| n.to_lowercase());
        v.dedup();
        v
    }

    pub fn has(&self, name: &str) -> bool {
        let k = name.trim().to_lowercase();
        self.registered.contains_key(&k) || self.installed.contains_key(&k)
    }

    /// Bumped on every change: a scene that sees a new number measures its text again.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The first family of the list that exists here; generic names end the search.
    pub fn resolve(&self, list: &[String]) -> FontChoice {
        for f in list {
            let k = f.trim().to_lowercase();
            if let Some(n) = self.registered.get(&k).or_else(|| self.installed.get(&k)) {
                return FontChoice::Named(n.clone());
            }
            match k.as_str() {
                "serif" | "ui-serif" => return FontChoice::Serif,
                "monospace" | "ui-monospace" => return FontChoice::Monospace,
                "sans-serif" | "system-ui" | "ui-sans-serif" | "-apple-system"
                | "blinkmacsystemfont" | "cursive" | "fantasy" => return FontChoice::Default,
                _ => {}
            }
        }
        FontChoice::Default
    }
}
