//! Small helpers over `roxmltree` that match elements and attributes by local name, whatever
//! namespace prefix the producer chose.

use roxmltree::{Document, Node, ParsingOptions};

use crate::ConvertError;

/// Most XML nodes one part may have.
const MAX_NODES: u32 = 8_000_000;
/// Deepest element nesting the readers walk (tables in tables, nested lists, spans).
pub(crate) const MAX_DEPTH: usize = 64;

/// Parse `text` (DTDs refused, node count capped); `what` names the part for errors.
pub(crate) fn parse<'i>(text: &'i str, what: &str) -> Result<Document<'i>, ConvertError> {
    let opts = ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES };
    Document::parse_with_options(text, opts).map_err(|e| ConvertError::Malformed(format!("{what} is not well-formed XML: {e}")))
}

/// Whether `n` is an element named `local` (any namespace).
pub(crate) fn is(n: Node, local: &str) -> bool {
    n.is_element() && n.tag_name().name() == local
}

/// The value of the attribute named `local` (any namespace).
pub(crate) fn attr<'a>(n: Node<'a, '_>, local: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == local).map(|a| a.value())
}

/// The first child element named `local`.
pub(crate) fn child<'a, 'i>(n: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is(*c, local))
}

/// The child elements named `local`.
pub(crate) fn children<'a, 'i: 'a>(n: Node<'a, 'i>, local: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    n.children().filter(move |c| is(*c, local))
}

/// The first descendant element named `local` (roxmltree walks the tree without recursion).
pub(crate) fn descendant<'a, 'i>(n: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.descendants().find(|d| is(*d, local))
}
