use tree_sitter::{Node, Tree};

pub enum Event<'tree> {
    Enter {
        node: Node<'tree>,
        field: Option<&'tree str>,
        leaf: bool,
    },
    Leave {
        node: Node<'tree>,
        leaf: bool,
    },
}

pub fn visible(node: Node<'_>) -> bool {
    node.is_named() || node.is_missing()
}

pub fn leaf(node: Node<'_>) -> bool {
    node.named_child_count() == 0 && !(node.has_error() && node.child_count() > 0)
}

pub fn walk<'tree, E>(
    tree: &'tree Tree,
    mut visit: impl FnMut(Event<'tree>) -> Result<(), E>,
) -> Result<(), E> {
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        let shown = visible(node);
        let leaf = leaf(node);
        if shown {
            let field = cursor.field_name();
            visit(Event::Enter { node, field, leaf })?;
        }
        let descend = (shown && !leaf) || (!shown && node.child_count() > 0);
        if descend && cursor.goto_first_child() {
            continue;
        }
        if shown && leaf {
            visit(Event::Leave { node, leaf: true })?;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(());
            }
            let parent = cursor.node();
            if visible(parent) {
                visit(Event::Leave { node: parent, leaf: false })?;
            }
        }
    }
}
