use std::borrow::Cow;
use std::io::{self, Write};
use std::str::FromStr;

use tree_sitter::{Node, Point, Tree};

use crate::error::Error;
use crate::walk::{Event, walk};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Sexp,
    Xml,
    Dot,
}

impl FromStr for Format {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "sexp" | "cst" => Ok(Self::Sexp),
            "xml" => Ok(Self::Xml),
            "dot" => Ok(Self::Dot),
            _ => Err(Error::UnknownFormat(name.to_owned())),
        }
    }
}

pub fn write(tree: &Tree, source: &[u8], format: Format, out: &mut impl Write) -> Result<(), Error> {
    match format {
        Format::Json => write_json(tree, source, out)?,
        Format::Sexp => writeln!(out, "{}", tree.root_node().to_sexp())?,
        Format::Xml => write_xml(tree, source, out)?,
        Format::Dot => write_dot(tree, source, out)?,
    }
    Ok(())
}

pub fn render(tree: &Tree, source: &[u8], format: Format) -> Result<String, Error> {
    let mut buffer = Vec::with_capacity(source.len() * 8);
    write(tree, source, format, &mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

pub fn text<'source>(node: Node<'_>, source: &'source [u8]) -> Cow<'source, str> {
    String::from_utf8_lossy(source.get(node.byte_range()).unwrap_or_default())
}

fn json_string(out: &mut impl Write, value: &str) -> io::Result<()> {
    serde_json::to_writer(&mut *out, value).map_err(io::Error::from)
}

fn json_point(out: &mut impl Write, key: &str, point: Point) -> io::Result<()> {
    write!(out, ",\"{key}\":[{},{}]", point.row, point.column)
}

fn closing(leaf: bool) -> &'static [u8] {
    if leaf {
        return b"}";
    }
    b"]}"
}

pub fn write_json(tree: &Tree, source: &[u8], out: &mut impl Write) -> io::Result<()> {
    let mut siblings: Vec<bool> = Vec::new();
    walk(tree, |event| -> io::Result<()> {
        let Event::Enter { node, field, leaf } = event else {
            siblings.pop();
            return out.write_all(closing(matches!(event, Event::Leave { leaf: true, .. })));
        };
        if let Some(seen) = siblings.last_mut() {
            if *seen {
                out.write_all(b",")?;
            }
            *seen = true;
        }
        out.write_all(b"{\"type\":")?;
        json_string(out, node.kind())?;
        if let Some(field) = field {
            out.write_all(b",\"field\":")?;
            json_string(out, field)?;
        }
        json_point(out, "start", node.start_position())?;
        json_point(out, "end", node.end_position())?;
        write!(out, ",\"bytes\":[{},{}]", node.start_byte(), node.end_byte())?;
        if node.is_error() {
            out.write_all(b",\"error\":true")?;
        }
        if node.is_missing() {
            out.write_all(b",\"missing\":true")?;
        }
        siblings.push(false);
        if !leaf {
            return out.write_all(b",\"children\":[");
        }
        out.write_all(b",\"text\":")?;
        json_string(out, &text(node, source))
    })?;
    out.write_all(b"\n")
}

fn xml_escape(value: &str) -> Cow<'_, str> {
    if !value.contains(['&', '<', '>', '"']) {
        return Cow::Borrowed(value);
    }
    Cow::Owned(
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;"),
    )
}

fn tag_character(character: char) -> char {
    if character.is_ascii_alphanumeric() || character == '_' {
        return character;
    }
    '_'
}

fn xml_tag(kind: &str) -> String {
    let tag: String = kind
        .chars()
        .map(tag_character)
        .collect();
    if tag.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_') {
        return tag;
    }
    format!("_{tag}")
}

pub fn write_xml(tree: &Tree, source: &[u8], out: &mut impl Write) -> io::Result<()> {
    let mut depth = 0usize;
    out.write_all(b"<?xml version=\"1.0\"?>\n")?;
    walk(tree, |event| -> io::Result<()> {
        let Event::Enter { node, field, leaf } = event else {
            depth = depth.saturating_sub(1);
            let Event::Leave { node, leaf: false } = event else {
                return Ok(());
            };
            return writeln!(out, "{}</{}>", "  ".repeat(depth), xml_tag(node.kind()));
        };
        let start = node.start_position();
        let end = node.end_position();
        write!(
            out,
            "{}<{} srow=\"{}\" scol=\"{}\" erow=\"{}\" ecol=\"{}\"",
            "  ".repeat(depth),
            xml_tag(node.kind()),
            start.row,
            start.column,
            end.row,
            end.column
        )?;
        if let Some(field) = field {
            write!(out, " field=\"{field}\"")?;
        }
        if node.is_error() {
            out.write_all(b" error=\"true\"")?;
        }
        if node.is_missing() {
            out.write_all(b" missing=\"true\"")?;
        }
        depth += 1;
        if !leaf {
            return out.write_all(b">\n");
        }
        writeln!(out, ">{}</{}>", xml_escape(&text(node, source)), xml_tag(node.kind()))
    })
}

pub fn write_dot(tree: &Tree, source: &[u8], out: &mut impl Write) -> io::Result<()> {
    let mut parents: Vec<usize> = Vec::new();
    out.write_all(b"digraph tree {\n  node [shape=box];\n")?;
    walk(tree, |event| -> io::Result<()> {
        let Event::Enter { node, field, leaf } = event else {
            parents.pop();
            return Ok(());
        };
        let id = node.id();
        let mut label = node.kind().escape_debug().to_string();
        if leaf {
            label = format!("{label}\\n{}", text(node, source).escape_debug());
        }
        writeln!(out, "  n{id} [label=\"{label}\"];")?;
        if let Some(parent) = parents.last() {
            writeln!(out, "  n{parent} -> n{id} [label=\"{}\"];", field.unwrap_or_default())?;
        }
        parents.push(id);
        Ok(())
    })?;
    out.write_all(b"}\n")
}
