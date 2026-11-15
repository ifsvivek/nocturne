//! Confirmation dialog state machine (pure). A write may only start when `activate` returns
//! `Out::Confirm`, which needs focus on the confirm button, Enter/click, and valid inputs.
use super::act::Act;

#[derive(Clone)]
pub struct Field {
    pub label: &'static str,
    pub value: String,
    pub secret: bool,
    pub numeric: bool,
    pub optional: bool,
    /// Must be typed exactly (delete-user guard).
    pub must_equal: Option<String>,
}

impl Field {
    pub fn new(label: &'static str, value: &str) -> Self {
        Self {
            label,
            value: value.into(),
            secret: false,
            numeric: false,
            optional: false,
            must_equal: None,
        }
    }

    fn ok(&self) -> bool {
        let v = self.value.trim();
        match &self.must_equal {
            Some(m) => v == m,
            None => self.optional || !v.is_empty(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Out {
    Stay,
    Cancel,
    Confirm,
}

pub struct Dialog {
    pub act: Act,
    pub title: String,
    pub body: String,
    pub confirm: &'static str,
    pub danger: bool,
    pub fields: Vec<Field>,
    /// 0..fields = inputs, then Cancel, then Confirm.
    pub focus: usize,
}

impl Dialog {
    pub fn new(act: Act, title: String, body: String, confirm: &'static str, danger: bool) -> Self {
        Self {
            act,
            title,
            body,
            confirm,
            danger,
            fields: vec![],
            focus: 0,
        }
    }

    pub fn field(mut self, f: Field) -> Self {
        self.fields.push(f);
        self
    }

    /// Cancel is the default focus unless there are inputs to fill first.
    pub fn start(mut self) -> Self {
        self.focus = if self.fields.is_empty() {
            self.cancel_ix()
        } else {
            0
        };
        self
    }

    pub fn cancel_ix(&self) -> usize {
        self.fields.len()
    }

    pub fn confirm_ix(&self) -> usize {
        self.fields.len() + 1
    }

    pub fn valid(&self) -> bool {
        self.fields.iter().all(Field::ok)
    }

    /// Enter / click on the focused element.
    pub fn activate(&mut self) -> Out {
        if self.focus < self.cancel_ix() {
            self.focus += 1; // finish an input, move on
            Out::Stay
        } else if self.focus == self.cancel_ix() {
            Out::Cancel
        } else if self.valid() {
            Out::Confirm
        } else {
            Out::Stay
        }
    }

    /// Key press (`key` as GPUI names it, `ch` the typed character, if any).
    pub fn key(&mut self, key: &str, ch: Option<&str>) -> Out {
        let n = self.fields.len() + 2;
        match key {
            "escape" => return Out::Cancel,
            "enter" => return self.activate(),
            "tab" | "down" => self.focus = (self.focus + 1) % n,
            "up" => self.focus = (self.focus + n - 1) % n,
            "left" | "right" if self.focus >= self.cancel_ix() => {
                self.focus = if self.focus == self.cancel_ix() {
                    self.confirm_ix()
                } else {
                    self.cancel_ix()
                }
            }
            "backspace" => {
                if let Some(f) = self.fields.get_mut(self.focus) {
                    f.value.pop();
                }
            }
            _ => {
                if let (Some(f), Some(c)) = (self.fields.get_mut(self.focus), ch)
                    && !c.chars().any(char::is_control)
                    && (!f.numeric || c.chars().all(|x| x.is_ascii_digit()))
                {
                    f.value.push_str(c);
                }
            }
        }
        Out::Stay
    }

    pub fn values(&self) -> Vec<String> {
        self.fields
            .iter()
            .map(|f| f.value.trim().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn del() -> Dialog {
        let mut f = Field::new("Type the name", "");
        f.must_equal = Some("bob".into());
        Dialog::new(Act::RefreshAll, "t".into(), "b".into(), "Delete", true)
            .field(f)
            .start()
    }

    fn plain() -> Dialog {
        Dialog::new(Act::RefreshAll, "t".into(), "b".into(), "Go", false).start()
    }

    #[test]
    fn cancel_is_default_and_enter_never_confirms_at_once() {
        let mut d = plain();
        assert_eq!(d.focus, d.cancel_ix());
        assert_eq!(d.key("enter", None), Out::Cancel);
        assert_eq!(plain().key("escape", None), Out::Cancel);
    }

    #[test]
    fn confirm_needs_focus_on_confirm() {
        let mut d = plain();
        assert_eq!(d.key("right", None), Out::Stay);
        assert_eq!(d.focus, d.confirm_ix());
        assert_eq!(d.key("enter", None), Out::Confirm);
        // moving back re-arms cancel
        let mut d = plain();
        d.key("right", None);
        d.key("left", None);
        assert_eq!(d.key("enter", None), Out::Cancel);
    }

    #[test]
    fn typed_name_gates_confirm() {
        let mut d = del();
        assert_eq!(
            d.key("enter", None),
            Out::Stay,
            "enter on an input just advances"
        );
        d.key("right", None);
        assert_eq!(d.focus, d.confirm_ix());
        assert_eq!(d.key("enter", None), Out::Stay, "empty name blocks");
        d.focus = 0;
        for c in ["b", "o", "x"] {
            d.key(c, Some(c));
        }
        d.focus = d.confirm_ix();
        assert_eq!(d.key("enter", None), Out::Stay, "wrong name blocks");
        d.focus = 0;
        d.key("backspace", None);
        d.key("b", Some("b"));
        d.focus = d.confirm_ix();
        assert_eq!(d.key("enter", None), Out::Confirm);
    }

    #[test]
    fn numeric_and_control_input() {
        let mut f = Field::new("days", "");
        f.numeric = true;
        let mut d = Dialog::new(Act::RefreshAll, "t".into(), "b".into(), "Save", false)
            .field(f)
            .start();
        d.key("a", Some("a"));
        d.key("7", Some("7"));
        d.key("x", Some("\u{7f}"));
        assert_eq!(d.fields[0].value, "7");
    }
}
