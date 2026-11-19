//! The element tree of one screen: NOCTURNE components with a tag, classes, an id and data attributes.
//! Nodes are stored parents first (a child is always added after its parent), so one forward pass can style and lay out the whole tree.
pub type Id = usize;

/// Hard cap per screen (see the limits in `docs/dev/THEME_ENGINE_PLAN.md`): CSS can never add elements, only the app does.
pub const MAX_NODES: usize = 50_000;

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
    pub children: Vec<Id>,
    /// Position among the parent's children (for `:nth-child`).
    pub index: u32,
}

#[derive(Clone, Debug)]
pub struct Doc {
    pub nodes: Vec<Node>,
}

impl Doc {
    pub fn new(tag: &str) -> Self {
        Self {
            nodes: vec![Node {
                tag: tag.into(),
                ..Node::default()
            }],
        }
    }

    /// Add a child. `spec` is `tag.class.class` (no spaces); an empty tag means `div`.
    pub fn add(&mut self, parent: Id, spec: &str) -> Id {
        let mut parts = spec.split('.');
        let tag = parts.next().filter(|t| !t.is_empty()).unwrap_or("div");
        let id = self.nodes.len();
        if id >= MAX_NODES {
            // Over the cap: park the node on the parent so callers keep working; the tree stops growing.
            return parent;
        }
        let index = self.nodes[parent].children.len() as u32;
        self.nodes.push(Node {
            tag: tag.into(),
            classes: parts.map(str::to_owned).collect(),
            parent: Some(parent),
            index,
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

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
