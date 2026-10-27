//! A small element tree for qbXML documents: QuickBooks' replies are a few levels deep and read
//! whole, so a tree is simpler than streaming.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    /// The element's own text, trimmed.
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The text at a `/`-separated path below this element, when it is there and not blank.
    pub fn text_at(&self, path: &str) -> Option<&str> {
        let mut node = self;
        for part in path.split('/') {
            node = node.child(part)?;
        }
        Some(node.text.as_str()).filter(|t| !t.is_empty())
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn start(e: &BytesStart) -> Result<Node, String> {
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| format!("QBXML_UNREADABLE: {e}"))?;
        let value = a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| format!("QBXML_UNREADABLE: {e}"))?;
        attrs.push((a.key.as_ref().to_string(), value.into_owned()));
    }
    Ok(Node { name: e.name().as_ref().to_string(), attrs, ..Default::default() })
}

/// Parses a document into its root element.
pub fn parse(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let close = |stack: &mut Vec<Node>, root: &mut Option<Node>, mut node: Node| {
        node.text = node.text.trim().to_string();
        match stack.last_mut() {
            Some(parent) => parent.children.push(node),
            None => *root = Some(node),
        }
    };
    loop {
        match reader.read_event().map_err(|e| format!("QBXML_UNREADABLE: QuickBooks' reply is not XML LocalBooks can read (at byte {}: {e})", reader.buffer_position()))? {
            Event::Start(e) => stack.push(start(&e)?),
            Event::Empty(e) => {
                let node = start(&e)?;
                close(&mut stack, &mut root, node);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or("QBXML_UNREADABLE: QuickBooks' reply closes an element it never opened")?;
                close(&mut stack, &mut root, node);
            }
            Event::Text(t) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if let Some(n) = stack.last_mut() {
                    n.text.push_str(&t);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(n) = stack.last_mut() {
                    let entity = format!("&{};", r.xml10_content());
                    match quick_xml::escape::unescape(&entity) {
                        Ok(s) => n.text.push_str(&s),
                        Err(_) => n.text.push_str(&entity),
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("QBXML_UNREADABLE: QuickBooks' reply ends before its elements close".into());
    }
    root.ok_or_else(|| "QBXML_UNREADABLE: QuickBooks sent back an empty reply".into())
}
