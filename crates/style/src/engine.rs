//! The engine's front door: a default sheet from the app, an optional user sheet layered over it, a time budget, `catch_unwind`
//! around user styles, a fallback to the default look, and a safe mode that ignores the user sheet altogether.
use crate::cascade::{Failure, Interaction};
use crate::doc::Doc;
use crate::fonts::Fonts;
use crate::layout::Scene;
use crate::sheet::{DEFAULT, Sheet, USER};
use crate::text::Measure;
use crate::virt::Builders;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::Duration;

/// Environment variable that starts the app without any user theme.
pub const SAFE_ENV: &str = "NOCTURNE_SAFE_THEME";
/// The key chord the app binds to [`Engine::toggle_safe_mode`] (handled before the tree sees key events).
pub const SAFE_CHORD: &str = "Ctrl+Alt+T";
/// Default time budget of one styling pass.
pub const DEFAULT_BUDGET: Duration = Duration::from_millis(50);
/// Consecutive overruns after which the user sheet is switched off.
pub const MAX_OVERRUNS: u8 = 3;

/// Does this value of [`SAFE_ENV`] ask for safe mode? Anything but unset, empty, `0` or `false`.
pub fn safe_env_enabled(v: Option<&str>) -> bool {
    v.is_some_and(|v| !matches!(v.trim(), "" | "0") && !v.trim().eq_ignore_ascii_case("false"))
}

pub struct Engine {
    default_css: String,
    default: Rc<Sheet>,
    layered: Option<Rc<Sheet>>,
    safe: bool,
    budget: Duration,
    overruns: u8,
    disabled: bool,
    pub fonts: Rc<Fonts>,
    notices: Vec<String>,
}

fn guard<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into())
    })
}

impl Engine {
    pub fn new(default_css: &str) -> Self {
        let safe = safe_env_enabled(std::env::var(SAFE_ENV).ok().as_deref());
        Self {
            default: Rc::new(Sheet::parse_default(default_css)),
            default_css: default_css.to_owned(),
            layered: None,
            safe,
            budget: DEFAULT_BUDGET,
            overruns: 0,
            disabled: false,
            fonts: Rc::new(Fonts::new()),
            notices: vec![],
        }
    }

    pub fn set_budget(&mut self, d: Duration) {
        self.budget = d;
    }

    /// Install (or remove) the user stylesheet; this also re-enables a sheet that was switched off for being slow.
    pub fn set_user_css(&mut self, css: Option<&str>) {
        self.layered =
            css.map(|c| Rc::new(Sheet::layered(&[(&self.default_css, DEFAULT), (c, USER)])));
        self.overruns = 0;
        self.disabled = false;
    }

    pub fn safe_mode(&self) -> bool {
        self.safe
    }

    pub fn set_safe_mode(&mut self, on: bool) {
        self.safe = on;
    }

    /// The key chord handler: returns the new state.
    pub fn toggle_safe_mode(&mut self) -> bool {
        self.safe = !self.safe;
        self.safe
    }

    pub fn fonts_mut(&mut self) -> &mut Fonts {
        Rc::make_mut(&mut self.fonts)
    }

    /// Messages for a banner (slow theme switched off, a panic caught, ...); drained by the call.
    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    /// The sheet that is in use now: the default alone in safe mode, when no user sheet is set, or when it was switched off.
    pub fn active_sheet(&self) -> Rc<Sheet> {
        match (&self.layered, self.safe || self.disabled) {
            (Some(l), false) => l.clone(),
            _ => self.default.clone(),
        }
    }

    pub fn user_sheet_active(&self) -> bool {
        self.layered.is_some() && !self.safe && !self.disabled
    }

    /// Build a scene. Never fails: a user sheet that panics or runs over budget is replaced by the default look (and, after
    /// three overruns, switched off with a notice).
    pub fn build(
        &mut self,
        doc: &Doc,
        viewport: (f32, f32),
        it: Interaction,
        builders: &Builders,
        measure: &mut Measure,
    ) -> Scene {
        if self.user_sheet_active() {
            let sheet = self.active_sheet();
            let fonts = self.fonts.clone();
            let d = doc.clone();
            let budget = self.budget;
            match guard(|| {
                Scene::build(
                    d,
                    sheet,
                    fonts,
                    viewport,
                    it,
                    Some(budget),
                    builders,
                    measure,
                )
            }) {
                Ok(Ok(s)) => {
                    if s.timing.cascade + s.timing.layout > self.budget {
                        // Built, but slow: kept for now; repeated slowness switches the theme off.
                        self.overruns += 1;
                        self.notices.push("the theme is slow to apply".into());
                        if self.overruns >= MAX_OVERRUNS {
                            self.switch_off();
                        }
                    } else {
                        self.overruns = 0;
                    }
                    return s;
                }
                Ok(Err(Failure::Overrun)) => self.overrun("the theme took too long to apply"),
                Ok(Err(Failure::Panic(m))) | Err(m) => {
                    self.notices.push(format!(
                        "the theme made the layout engine fail ({m}); the default look is shown"
                    ));
                    self.overrun("");
                }
            }
        }
        self.build_default(doc, viewport, it, builders, measure)
    }

    fn overrun(&mut self, why: &str) {
        self.overruns += 1;
        if !why.is_empty() {
            self.notices
                .push(format!("{why}; the default look is shown"));
        }
        if self.overruns >= MAX_OVERRUNS {
            self.switch_off();
        }
    }

    fn switch_off(&mut self) {
        self.disabled = true;
        self.notices.push(format!(
            "the theme was switched off after {MAX_OVERRUNS} slow or failed attempts (press {SAFE_CHORD} or set it again to retry)"
        ));
    }

    fn build_default(
        &mut self,
        doc: &Doc,
        viewport: (f32, f32),
        it: Interaction,
        builders: &Builders,
        measure: &mut Measure,
    ) -> Scene {
        let attempts = [self.default.clone(), Rc::new(Sheet::default())];
        for sheet in attempts {
            let d = doc.clone();
            let fonts = self.fonts.clone();
            if let Ok(Ok(s)) =
                guard(|| Scene::build(d, sheet, fonts, viewport, it, None, builders, measure))
            {
                return s;
            }
        }
        // The bare sheet cannot fail on a valid tree; reaching this means the tree itself is broken.
        Scene::build(
            Doc::default(),
            Rc::new(Sheet::default()),
            self.fonts.clone(),
            viewport,
            it,
            None,
            &Builders::new(),
            measure,
        )
        .unwrap_or_else(|_| unreachable!("an empty document always builds"))
    }
}
