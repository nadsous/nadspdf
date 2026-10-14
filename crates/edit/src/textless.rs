//! A copy of a document that draws no text: what each page shows behind its words. Export a
//! PDF ▸ Word lays a picture of it under editable text boxes, so backgrounds, artwork and rules
//! keep their look while the words stay editable.

use std::collections::HashSet;

use pdfcraft_content::{Op, parse, serialize_ops};
use pdfcraft_cos::{Dict, Document, ObjRef, Object, Stream};

/// How deep form XObjects are followed; deeper ones keep their text.
const MAX_DEPTH: usize = 16;
/// The most streams rewritten, so a file with a huge number of forms stays bounded.
const MAX_STREAMS: usize = 20_000;

fn shows_text(op: &Op) -> bool {
    op.is("Tj") || op.is("TJ") || op.is("'") || op.is("\"")
}

/// `doc` with every text-showing operator removed from its pages' content and from the form
/// XObjects they draw (Type 3 glyphs go with the text that shows them). Annotation appearances
/// are left as they are. The copy is never encrypted; `doc` itself is untouched.
pub fn without_text(doc: &Document) -> Document {
    let mut out = doc.clone();
    out.remove_encryption();
    let mut seen = HashSet::new();
    for page in crate::page_list(&out) {
        let refs: Vec<ObjRef> = match page.dict.get(b"Contents") {
            Some(Object::Ref(r)) => match out.get(*r).as_array() {
                Some(a) => a.iter().filter_map(|o| if let Object::Ref(r) = o { Some(*r) } else { None }).collect(),
                None => vec![*r],
            },
            Some(Object::Array(a)) => a.iter().filter_map(|o| if let Object::Ref(r) = o { Some(*r) } else { None }).collect(),
            _ => Vec::new(),
        };
        for r in refs {
            strip(&mut out, r, &mut seen);
        }
        if let Some(res) = page.dict.get(b"Resources").and_then(|r| out.resolve(r).as_dict().cloned()) {
            forms(&mut out, &res, 0, &mut seen);
        }
    }
    out
}

/// Strip the form XObjects `resources` names, and the forms they draw in turn.
fn forms(doc: &mut Document, resources: &Dict, depth: usize, seen: &mut HashSet<ObjRef>) {
    if depth >= MAX_DEPTH {
        return;
    }
    let Some(xobjects) = resources.get(b"XObject").and_then(|x| doc.resolve(x).as_dict().cloned()) else { return };
    for (_, o) in xobjects.iter() {
        let Object::Ref(r) = o else { continue };
        if seen.contains(r) || seen.len() >= MAX_STREAMS {
            continue;
        }
        let inner = match &*doc.get(*r) {
            Object::Stream(s) if s.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form".as_slice()) => {
                s.dict.get(b"Resources").and_then(|x| doc.resolve(x).as_dict().cloned())
            }
            _ => continue,
        };
        strip(doc, *r, seen);
        if let Some(inner) = inner {
            forms(doc, &inner, depth + 1, seen);
        }
    }
}

/// Rewrite content stream `r` without its text-showing operators. A stream that shows no text,
/// or can't be decoded, is left byte for byte.
fn strip(doc: &mut Document, r: ObjRef, seen: &mut HashSet<ObjRef>) {
    if seen.len() >= MAX_STREAMS || !seen.insert(r) {
        return;
    }
    let obj = doc.get(r);
    let Object::Stream(s) = &*obj else { return };
    let Ok(data) = s.decoded() else { return };
    let parsed = parse(&data);
    if !parsed.ops.iter().any(shows_text) {
        return;
    }
    let kept: Vec<Op> = parsed.ops.into_iter().filter(|op| !shows_text(op)).collect();
    let mut dict = s.dict.clone();
    dict.remove(b"Length");
    doc.set(r, Object::Stream(Stream::flate(dict, &serialize_ops(&kept))));
}
