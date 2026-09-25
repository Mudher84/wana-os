//! Wana's Wayland protocol scanner: reads protocol XML (wayland.xml,
//! xdg-shell.xml, ...) and generates the Rust source of the `wl_interface`
//! tables libwayland needs, plus opcode constants.
//!
//! Used by `build.rs` (at build time) and by the unit tests. It must not
//! depend on the rest of the crate. The table rules follow
//! wayland-scanner's `private-code` output, and a unit test compares the
//! generated core tables with the ones libwayland-server itself exports.

use std::collections::HashMap;
use std::fmt::Write as _;

/// Argument wire types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgType {
    Int,
    Uint,
    Fixed,
    String,
    Object,
    NewId,
    Array,
    Fd,
}

impl ArgType {
    fn parse(s: &str) -> Result<ArgType, String> {
        Ok(match s {
            "int" => ArgType::Int,
            "uint" => ArgType::Uint,
            "fixed" => ArgType::Fixed,
            "string" => ArgType::String,
            "object" => ArgType::Object,
            "new_id" => ArgType::NewId,
            "array" => ArgType::Array,
            "fd" => ArgType::Fd,
            other => return Err(format!("unknown argument type {other:?}")),
        })
    }

    fn code(self) -> char {
        match self {
            ArgType::Int => 'i',
            ArgType::Uint => 'u',
            ArgType::Fixed => 'f',
            ArgType::String => 's',
            ArgType::Object => 'o',
            ArgType::NewId => 'n',
            ArgType::Array => 'a',
            ArgType::Fd => 'h',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    pub name: String,
    pub ty: ArgType,
    pub interface: Option<String>,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub name: String,
    pub since: u32,
    pub destructor: bool,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub version: u32,
    pub requests: Vec<Message>,
    pub events: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protocol {
    pub name: String,
    pub interfaces: Vec<Interface>,
}

impl Message {
    /// libwayland signature: optional `since` version, then one type code per
    /// argument, `?` before nullable ones; an untyped new_id expands to `sun`
    /// (interface name, version, id), as in wl_registry.bind.
    pub fn signature(&self) -> String {
        let mut sig = String::new();
        if self.since > 1 {
            sig.push_str(&self.since.to_string());
        }
        for a in &self.args {
            if a.nullable {
                sig.push('?');
            }
            if a.ty == ArgType::NewId && a.interface.is_none() {
                sig.push_str("su");
            }
            sig.push(a.ty.code());
        }
        sig
    }

    /// The `types` entries: one per wire value; the interface name for typed
    /// object/new_id arguments, None otherwise.
    pub fn types(&self) -> Vec<Option<&str>> {
        let mut t = Vec::new();
        for a in &self.args {
            if a.ty == ArgType::NewId && a.interface.is_none() {
                t.extend([None, None]);
            }
            match a.ty {
                ArgType::Object | ArgType::NewId => t.push(a.interface.as_deref()),
                _ => t.push(None),
            }
        }
        t
    }
}

// --- XML ------------------------------------------------------------------

/// One element start or end, with attributes. Text, comments, the XML
/// declaration and DOCTYPE are skipped: protocol files only carry
/// documentation there.
#[derive(Debug, PartialEq, Eq)]
enum Tag {
    Start {
        name: String,
        attrs: Vec<(String, String)>,
        empty: bool,
    },
    End {
        name: String,
    },
}

fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let end = rest[i..]
            .find(';')
            .ok_or_else(|| format!("unterminated entity in {s:?}"))?;
        let ent = &rest[i + 1..i + end];
        out.push(match ent {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = ent
                    .strip_prefix("#x")
                    .map(|h| u32::from_str_radix(h, 16))
                    .or_else(|| ent.strip_prefix('#').map(str::parse))
                    .ok_or_else(|| format!("unknown entity &{ent};"))?
                    .map_err(|e| format!("bad entity &{ent};: {e}"))?;
                char::from_u32(code).ok_or_else(|| format!("bad character &{ent};"))?
            }
        });
        rest = &rest[i + end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn tokenize(xml: &str) -> Result<Vec<Tag>, String> {
    let mut tags = Vec::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt..];
        let skip_to = |rest: &str, end: &str| -> Result<usize, String> {
            rest.find(end)
                .map(|i| i + end.len())
                .ok_or_else(|| format!("unterminated {:?}", &rest[..rest.len().min(20)]))
        };
        if rest.starts_with("<!--") {
            rest = &rest[skip_to(rest, "-->")?..];
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            rest = &rest[skip_to(rest, "]]>")?..];
            continue;
        }
        if rest.starts_with("<?") {
            rest = &rest[skip_to(rest, "?>")?..];
            continue;
        }
        if rest.starts_with("<!") {
            rest = &rest[skip_to(rest, ">")?..];
            continue;
        }
        // An element tag; attribute values may contain '>', so scan quotes.
        let bytes = rest.as_bytes();
        let mut i = 1;
        let mut quote = None;
        while i < bytes.len() {
            match (quote, bytes[i]) {
                (None, b'"' | b'\'') => quote = Some(bytes[i]),
                (Some(q), c) if c == q => quote = None,
                (None, b'>') => break,
                _ => {}
            }
            i += 1;
        }
        if i >= bytes.len() {
            return Err("unterminated tag".into());
        }
        let inner = &rest[1..i];
        rest = &rest[i + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            tags.push(Tag::End {
                name: name.trim().to_owned(),
            });
            continue;
        }
        let (inner, empty) = match inner.strip_suffix('/') {
            Some(s) => (s, true),
            None => (inner, false),
        };
        let name_end = inner
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_owned();
        let mut attrs = Vec::new();
        let mut a = inner[name_end..].trim_start();
        while !a.is_empty() {
            let eq = a
                .find('=')
                .ok_or_else(|| format!("<{name}>: attribute without value"))?;
            let key = a[..eq].trim().to_owned();
            let v = a[eq + 1..].trim_start();
            let q = v
                .chars()
                .next()
                .filter(|c| *c == '"' || *c == '\'')
                .ok_or_else(|| format!("<{name}>: unquoted value for {key}"))?;
            let close = v[1..]
                .find(q)
                .ok_or_else(|| format!("<{name}>: unterminated value for {key}"))?;
            attrs.push((key, unescape(&v[1..1 + close])?));
            a = v[close + 2..].trim_start();
        }
        tags.push(Tag::Start { name, attrs, empty });
    }
    Ok(tags)
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn required<'a>(attrs: &'a [(String, String)], key: &str, elem: &str) -> Result<&'a str, String> {
    attr(attrs, key).ok_or_else(|| format!("<{elem}> without {key}"))
}

fn number(v: &str, what: &str) -> Result<u32, String> {
    v.parse().map_err(|e| format!("{what} {v:?}: {e}"))
}

/// Parses one protocol XML file.
pub fn parse(xml: &str) -> Result<Protocol, String> {
    let mut protocol: Option<Protocol> = None;
    let mut interface: Option<Interface> = None;
    // (is_request, message)
    let mut message: Option<(bool, Message)> = None;
    let mut depth_stack: Vec<String> = Vec::new();

    for tag in tokenize(xml)? {
        match tag {
            Tag::Start { name, attrs, empty } => {
                match name.as_str() {
                    "protocol" => {
                        protocol = Some(Protocol {
                            name: required(&attrs, "name", "protocol")?.to_owned(),
                            interfaces: Vec::new(),
                        })
                    }
                    "interface" => {
                        interface = Some(Interface {
                            name: required(&attrs, "name", "interface")?.to_owned(),
                            version: number(
                                required(&attrs, "version", "interface")?,
                                "interface version",
                            )?,
                            requests: Vec::new(),
                            events: Vec::new(),
                        })
                    }
                    "request" | "event" => {
                        let m = Message {
                            name: required(&attrs, "name", &name)?.to_owned(),
                            since: attr(&attrs, "since")
                                .map(|v| number(v, "since"))
                                .transpose()?
                                .unwrap_or(1),
                            destructor: attr(&attrs, "type") == Some("destructor"),
                            args: Vec::new(),
                        };
                        if empty {
                            push_message(&mut interface, name == "request", m)?;
                        } else {
                            message = Some((name == "request", m));
                        }
                    }
                    "arg" => {
                        let (_, m) = message.as_mut().ok_or("<arg> outside a request or event")?;
                        m.args.push(Arg {
                            name: required(&attrs, "name", "arg")?.to_owned(),
                            ty: ArgType::parse(required(&attrs, "type", "arg")?)?,
                            interface: attr(&attrs, "interface").map(str::to_owned),
                            nullable: attr(&attrs, "allow-null") == Some("true"),
                        });
                    }
                    _ => {}
                }
                if !empty {
                    depth_stack.push(name);
                }
            }
            Tag::End { name } => {
                let open = depth_stack
                    .pop()
                    .ok_or_else(|| format!("unexpected </{name}>"))?;
                if open != name {
                    return Err(format!("</{name}> closes <{open}>"));
                }
                match name.as_str() {
                    "request" | "event" => {
                        let (is_request, m) = message.take().ok_or("stray message end")?;
                        push_message(&mut interface, is_request, m)?;
                    }
                    "interface" => {
                        let i = interface.take().ok_or("stray </interface>")?;
                        protocol
                            .as_mut()
                            .ok_or("<interface> outside <protocol>")?
                            .interfaces
                            .push(i);
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(open) = depth_stack.pop() {
        return Err(format!("<{open}> is not closed"));
    }
    protocol.ok_or_else(|| "no <protocol> element".into())
}

fn push_message(
    interface: &mut Option<Interface>,
    is_request: bool,
    m: Message,
) -> Result<(), String> {
    let i = interface
        .as_mut()
        .ok_or("<request>/<event> outside <interface>")?;
    if is_request {
        i.requests.push(m);
    } else {
        i.events.push(m);
    }
    Ok(())
}

// --- Code generation -------------------------------------------------------

/// `wl_surface` -> `WL_SURFACE`.
fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

/// Protocol name to a Rust module name (`xdg-shell` -> `xdg_shell`).
pub fn module_name(protocol: &str) -> String {
    protocol.replace('-', "_")
}

/// Generates Rust source for the given protocols. Interfaces referenced by
/// arguments must be defined in one of them.
pub fn generate(protocols: &[Protocol]) -> Result<String, String> {
    let mut owner: HashMap<&str, String> = HashMap::new();
    for p in protocols {
        for i in &p.interfaces {
            if owner.insert(&i.name, module_name(&p.name)).is_some() {
                return Err(format!("interface {} defined twice", i.name));
            }
        }
    }
    let mut out = String::new();
    out.push_str("// Generated by wana-wayland's scanner (src/scanner.rs). Do not edit.\n\n");
    for p in protocols {
        let module = module_name(&p.name);
        writeln!(out, "/// Protocol `{}`.", p.name).unwrap();
        writeln!(out, "pub mod {module} {{").unwrap();
        out.push_str("    #![allow(clippy::all)]\n");
        out.push_str("    use crate::sys::{wl_interface, wl_message, TypeRef};\n\n");
        for i in &p.interfaces {
            let base = upper(&i.name);
            for (kind, msgs) in [("REQUESTS", &i.requests), ("EVENTS", &i.events)] {
                for (n, m) in msgs.iter().enumerate() {
                    let types = m.types();
                    let entries: Vec<String> = types
                        .iter()
                        .map(|t| match t {
                            None => Ok("TypeRef::NULL".to_owned()),
                            Some(name) => {
                                let module = owner.get(name).ok_or_else(|| {
                                    format!("{}.{}: unknown interface {name}", i.name, m.name)
                                })?;
                                Ok(format!(
                                    "TypeRef(&super::{module}::{}_INTERFACE)",
                                    upper(name)
                                ))
                            }
                        })
                        .collect::<Result<_, String>>()?;
                    writeln!(
                        out,
                        "    static {base}_{kind}_{n}_TYPES: [TypeRef; {}] = [{}];",
                        entries.len(),
                        entries.join(", ")
                    )
                    .unwrap();
                }
                let list: Vec<String> = msgs
                    .iter()
                    .enumerate()
                    .map(|(n, m)| {
                        format!(
                            "        wl_message {{ name: c\"{}\".as_ptr(), signature: c\"{}\".as_ptr(), types: {base}_{kind}_{n}_TYPES.as_ptr() }},\n",
                            m.name,
                            m.signature()
                        )
                    })
                    .collect();
                writeln!(
                    out,
                    "    static {base}_{kind}: [wl_message; {}] = [\n{}    ];",
                    msgs.len(),
                    list.concat()
                )
                .unwrap();
            }
            writeln!(
                out,
                "    /// `{name}` version {v}.\n    pub static {base}_INTERFACE: wl_interface = wl_interface {{\n        name: c\"{name}\".as_ptr(),\n        version: {v},\n        method_count: {rc},\n        methods: {base}_REQUESTS.as_ptr(),\n        event_count: {ec},\n        events: {base}_EVENTS.as_ptr(),\n    }};",
                name = i.name,
                v = i.version,
                rc = i.requests.len(),
                ec = i.events.len(),
            )
            .unwrap();
            writeln!(out, "    /// Opcodes and version of `{}`.", i.name).unwrap();
            writeln!(out, "    pub mod {} {{", i.name).unwrap();
            writeln!(out, "        pub const VERSION: u32 = {};", i.version).unwrap();
            for (kind, msgs) in [("request", &i.requests), ("event", &i.events)] {
                writeln!(out, "        pub mod {kind} {{").unwrap();
                for (n, m) in msgs.iter().enumerate() {
                    writeln!(out, "            pub const {}: u32 = {n};", upper(&m.name)).unwrap();
                }
                out.push_str("        }\n");
            }
            out.push_str("    }\n\n");
        }
        let all: Vec<String> = p
            .interfaces
            .iter()
            .map(|i| format!("&{}_INTERFACE", upper(&i.name)))
            .collect();
        writeln!(
            out,
            "    /// Every interface of this protocol.\n    pub static INTERFACES: [&wl_interface; {}] = [{}];",
            all.len(),
            all.join(", ")
        )
        .unwrap();
        out.push_str("}\n\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<protocol name="sample-shell">
  <copyright>a &lt;b&gt; c</copyright>
  <!-- <interface name="ignored" version="9"/> -->
  <interface name="wl_registry" version="1">
    <description summary="x">Text with > and &amp;.</description>
    <request name="bind">
      <arg name="name" type="uint" summary="a &quot;name&quot;"/>
      <arg name="id" type="new_id"/>
    </request>
    <event name="global_remove"><arg name="name" type="uint"/></event>
  </interface>
  <interface name="thing" version="3">
    <request name="destroy" type="destructor"/>
    <request name="attach" since="2">
      <arg name="buffer" type="object" interface="thing" allow-null="true"/>
      <arg name="x" type="int"/><arg name="fd" type="fd"/>
      <arg name="title" type="string" allow-null='true'/>
    </request>
    <event name="done"><arg name="child" type="new_id" interface="thing"/></event>
  </interface>
</protocol>"#;

    #[test]
    fn parses_a_protocol() {
        let p = parse(SAMPLE).unwrap();
        assert_eq!(p.name, "sample-shell");
        assert_eq!(p.interfaces.len(), 2, "commented-out interface is skipped");
        let reg = &p.interfaces[0];
        assert_eq!((reg.requests.len(), reg.events.len()), (1, 1));
        let thing = &p.interfaces[1];
        assert_eq!(thing.version, 3);
        assert!(thing.requests[0].destructor);
        assert_eq!(thing.requests[1].since, 2);
        assert_eq!(thing.requests[1].args.len(), 4);
    }

    #[test]
    fn signatures_and_types_follow_wayland_scanner() {
        let p = parse(SAMPLE).unwrap();
        let bind = &p.interfaces[0].requests[0];
        assert_eq!(bind.signature(), "usun", "untyped new_id expands to sun");
        assert_eq!(bind.types(), vec![None, None, None, None]);
        let attach = &p.interfaces[1].requests[1];
        assert_eq!(attach.signature(), "2?oih?s");
        assert_eq!(attach.types(), vec![Some("thing"), None, None, None]);
        let done = &p.interfaces[1].events[0];
        assert_eq!(
            (done.signature().as_str(), done.types()),
            ("n", vec![Some("thing")])
        );
        assert_eq!(p.interfaces[1].requests[0].signature(), "");
    }

    #[test]
    fn malformed_input_is_an_error() {
        assert!(parse("<protocol name=\"x\"><interface name=\"a\" version=\"1\">").is_err());
        assert!(parse("<protocol name=\"x\"></interface></protocol>").is_err());
        assert!(parse("<protocol name=\"x\"><interface version=\"1\"/></protocol>").is_err());
        let bad_type = "<protocol name=\"x\"><interface name=\"a\" version=\"1\"><request name=\"r\"><arg name=\"v\" type=\"float\"/></request></interface></protocol>";
        assert!(parse(bad_type).unwrap_err().contains("float"));
    }

    #[test]
    fn unknown_interface_reference_is_an_error() {
        let p = parse(SAMPLE).unwrap();
        let mut broken = p.clone();
        broken.interfaces[1].events[0].args[0].interface = Some("nowhere".into());
        assert!(generate(&[broken]).unwrap_err().contains("nowhere"));
        assert!(generate(&[p]).unwrap().contains("pub mod sample_shell {"));
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(unescape("a &lt;&amp;&gt; &#65;&#x42;").unwrap(), "a <&> AB");
        assert!(unescape("&nope;").is_err());
    }
}
