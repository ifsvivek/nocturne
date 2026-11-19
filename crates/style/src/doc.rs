//! The element tree of one screen: NOCTURNE components with a tag, classes, an id and data attributes.
//! Nodes are stored parents first (a child is always added after its parent), so one forward pass can style and lay out the whole tree.
//! CSS can never add elements; only the app's tree builders can (plus bounded `::before`/`::after` text leaves, see `Node::generated`).
pub type Id = usize;

/// Hard caps per screen (see the limits in `docs/dev/THEME_ENGINE_PLAN.md`).
pub const MAX_NODES: usize = 50_000;
/// Taffy and the cascade recurse over the tree; a bound keeps the stack safe whatever the builders do.
pub const MAX_DEPTH: u16 = 96;

/// Which pseudo-element a generated node stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gen {
    Before,
    After,
}

/// An image the app supplies by id (its pixels and handle live in the widget).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageRef {
    pub id: u32,
    pub w: u32,
    pub h: u32,
}

/// A long list that lays out only the items near the viewport (see `virt.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Virtual {
    pub count: usize,
    /// Rows instantiated above and below the visible ones.
    pub overscan_rows: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<(String, String)>,
    /// A leaf with text: the box is measured from this string.
    pub text: Option<String>,
    /// Style the app itself sets from data (artwork colours, a card width): highest specificity, below `!important`.
    pub inline: Option<String>,
    pub parent: Option<Id>,
    /// Real children only; generated `::before`/`::after` leaves are `before` and `after`.
    pub children: Vec<Id>,
    /// Position among the parent's real children (for `:nth-child`).
    pub index: u32,
    pub depth: u16,
    /// The app decides what can take keyboard focus, never CSS.
    pub focusable: bool,
    /// Back, search, settings, player controls: a theme cannot hide, fade, shrink or move these off screen.
    pub essential: bool,
    /// A real Iced element is hosted at this node's rectangle (`widget/styled.rs`).
    pub slot: Option<u32>,
    /// A replaced element: an image the app draws (`widget/styled.rs`); the box is measured from its intrinsic size.
    pub image: Option<ImageRef>,
    pub virt: Option<Virtual>,
    /// The one example item of a virtual container: styled for its metrics, never shown.
    pub virtual_proto: bool,
    /// Set on the items a virtual container instantiated for its visible window.
    pub virtual_item: bool,
    pub generated: Option<Gen>,
    pub before: Option<Id>,
    pub after: Option<Id>,
}

#[derive(Clone, Debug)]
pub struct Doc {
    pub nodes: Vec<Node>,
    /// Nodes that were not added because a limit was reached.
    pub dropped: usize,
}

impl Default for Doc {
    fn default() -> Self {
        Doc::new("page")
    }
}

impl Doc {
    pub fn new(tag: &str) -> Self {
        Self {
            nodes: vec![Node {
                tag: tag.into(),
                ..Node::default()
            }],
            dropped: 0,
        }
    }

    /// Add a child. `spec` is `tag.class.class` (no spaces); an empty tag means `div`.
    pub fn add(&mut self, parent: Id, spec: &str) -> Id {
        let mut parts = spec.split('.');
        let tag = parts.next().filter(|t| !t.is_empty()).unwrap_or("div");
        let id = self.nodes.len();
        let depth = self.nodes[parent].depth + 1;
        if id >= MAX_NODES || depth > MAX_DEPTH {
            // Over a cap: park the node on the parent so callers keep working; the tree stops growing.
            self.dropped += 1;
            return parent;
        }
        let index = self.nodes[parent].children.len() as u32;
        self.nodes.push(Node {
            tag: tag.into(),
            classes: parts.filter(|c| !c.is_empty()).map(str::to_owned).collect(),
            parent: Some(parent),
            index,
            depth,
            ..Node::default()
        });
        self.nodes[parent].children.push(id);
        id
    }

    pub fn attr(&mut self, n: Id, k: &str, v: &str) -> Id {
        self.nodes[n].attrs.push((k.into(), v.into()));
        n
    }

    pub fn text(&mut self, n: Id, t: &str) -> Id {
        self.nodes[n].text = Some(t.into());
        n
    }

    pub fn inline(&mut self, n: Id, css: &str) -> Id {
        self.nodes[n].inline = Some(css.into());
        n
    }

    pub fn set_id(&mut self, n: Id, id: &str) -> Id {
        self.nodes[n].id = Some(id.into());
        n
    }

    pub fn focusable(&mut self, n: Id) -> Id {
        self.nodes[n].focusable = true;
        n
    }

    /// Mark an element the theme cannot take away; also exposes `[data-essential]` to selectors.
    pub fn essential(&mut self, n: Id) -> Id {
        self.nodes[n].essential = true;
        if !self.nodes[n].attrs.iter().any(|(k, _)| k == "data-essential") {
            self.nodes[n].attrs.push(("data-essential".into(), String::new()));
        }
        n
    }

    pub fn slot(&mut self, n: Id, slot: u32) -> Id {
        self.nodes[n].slot = Some(slot);
        n
    }

    pub fn image(&mut self, n: Id, id: u32, w: u32, h: u32) -> Id {
        self.nodes[n].image = Some(ImageRef { id, w, h });
        n
    }

    /// Mark `n` (a child of a virtual container, built like every item) as the container's prototype item.
    pub fn virtual_proto(&mut self, n: Id) -> Id {
        self.nodes[n].virtual_proto = true;
        n
    }

    /// Turn `n` into a virtual container of `count` items (the app supplies the item builder to the scene).
    pub fn virtual_list(&mut self, n: Id, count: usize) -> Id {
        self.nodes[n].virt = Some(Virtual {
            count,
            overscan_rows: 2,
        });
        n
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// `tag#id.class.class`, for messages.
    pub fn describe(&self, n: Id) -> String {
        let node = &self.nodes[n];
        let mut s = node.tag.clone();
        if let Some(i) = &node.id {
            s.push('#');
            s.push_str(i);
        }
        for c in &node.classes {
            s.push('.');
            s.push_str(c);
        }
        match node.generated {
            Some(Gen::Before) => s.push_str("::before"),
            Some(Gen::After) => s.push_str("::after"),
            None => {}
        }
        s
    }

    /// Remove every node from `keep` on (generated leaves and virtual items), restoring the links of the survivors.
    pub fn truncate(&mut self, keep: usize) {
        if self.nodes.len() <= keep {
            return;
        }
        let parents: Vec<Id> = self.nodes[keep..]
            .iter()
            .filter_map(|n| n.parent)
            .filter(|&p| p < keep)
            .collect();
        self.nodes.truncate(keep);
        for p in parents {
            let n = &mut self.nodes[p];
            n.children.retain(|&c| c < keep);
        }
        for n in &mut self.nodes {
            if n.before.is_some_and(|b| b >= keep) {
                n.before = None;
            }
            if n.after.is_some_and(|a| a >= keep) {
                n.after = None;
            }
        }
    }

    /// Previous real sibling.
    pub fn prev_sibling(&self, n: Id) -> Option<Id> {
        let node = &self.nodes[n];
        let p = node.parent?;
        (node.index > 0).then(|| self.nodes[p].children[node.index as usize - 1])
    }

    pub fn next_sibling(&self, n: Id) -> Option<Id> {
        let node = &self.nodes[n];
        let p = node.parent?;
        self.nodes[p].children.get(node.index as usize + 1).copied()
    }
}
