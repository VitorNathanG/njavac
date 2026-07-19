use super::reader::{dump, raw, Field};

/// A "derived" field is a count or byte-length fully determined by the content
/// that follows it — when it differs, the divergence is a *consequence*, not the
/// cause, so the differ demotes it and headlines the substantive field instead.
/// NOTE: `max_stack`/`max_locals` are deliberately NOT derived — they are computed
/// verifier values, and a difference there is a genuine root cause.
fn is_derived(path: &str) -> bool {
    path.ends_with("_count") // constant_pool_count, *_count, attributes_count
        || path.ends_with(".length") // {attr}.length, cp[N].length (Utf8 length)
        || path.ends_with("_length") // code_length, exception_table_length, table_length
        || path.ends_with("number_of_entries")
        || path.ends_with("number_of_locals")
        || path.ends_with("number_of_stack_items")
}

/// Compare two class files structurally. Returns `None` if they are byte-identical,
/// or `Some(report)` describing the first divergence (structural when both parse,
/// otherwise a raw byte window). The report always leads with the first differing
/// byte offset — the ground truth that localizes even *within* a raw field.
pub fn diff_report(a: &[u8], b: &[u8]) -> Option<String> {
    if a == b {
        return None;
    }
    let mut out = String::new();
    out.push_str(&format!(
        "class files differ: javac={} bytes, njavac={} bytes\n",
        a.len(),
        b.len()
    ));
    match first_differing_byte(a, b) {
        Some(i) => out.push_str(&format!("first differing byte: {}\n", off(i))),
        None => out.push_str(&format!(
            "one file is a prefix of the other; they diverge at byte {}\n",
            off(a.len().min(b.len()))
        )),
    }

    match (dump(a), dump(b)) {
        (Ok(fa), Ok(fb)) => {
            let n = fa.len().max(fb.len());
            let diffs: Vec<usize> =
                (0..n).filter(|&i| field_differs(fa.get(i), fb.get(i))).collect();
            // Prefer the first SUBSTANTIVE (non-derived) divergence: a derived
            // count/length differs only as a *consequence* of the content it
            // measures, so headlining it (`constant_pool_count`, `attr[..].length`)
            // buries the cause. Fall back to the first divergence if all are derived.
            let path_at =
                |i: usize| fa.get(i).or(fb.get(i)).map(|f| f.path.as_str()).unwrap_or("");
            let substantive = diffs.iter().copied().find(|&i| !is_derived(path_at(i)));
            let idx = substantive.or_else(|| diffs.first().copied());
            match idx {
                Some(i) => {
                    out.push('\n');
                    // A derived field that diverged earlier is named as a consequence,
                    // so the count/length change stays visible but is not the headline.
                    if let Some(&d) = diffs.first() {
                        if d != i && is_derived(path_at(d)) {
                            let da = fa.get(d).map_or("∅".to_string(), |f| clip(&f.value).to_string());
                            let db = fb.get(d).map_or("∅".to_string(), |f| clip(&f.value).to_string());
                            out.push_str(&format!(
                                "derived {} differs (javac {da} / njavac {db}) — a \
                                 consequence of the divergence below\n",
                                path_at(d)
                            ));
                        }
                    }
                    out.push_str("first structural divergence:\n");
                    match (fa.get(i), fb.get(i)) {
                        (Some(x), Some(y)) => {
                            out.push_str(&format!("  path   : {}\n", x.path));
                            out.push_str(&format!("  offset : {}\n", off(x.offset)));
                            out.push_str(&format!("  javac  : {}\n", clip(&x.value)));
                            out.push_str(&format!("  njavac : {}\n", clip(&y.value)));
                        }
                        (Some(x), None) => {
                            out.push_str(&format!(
                                "  njavac ended early; javac still has:\n    {} @ {} = {}\n",
                                x.path,
                                off(x.offset),
                                clip(&x.value)
                            ));
                        }
                        (None, Some(y)) => {
                            out.push_str(&format!(
                                "  javac ended early; njavac still has:\n    {} @ {} = {}\n",
                                y.path,
                                off(y.offset),
                                clip(&y.value)
                            ));
                        }
                        (None, None) => unreachable!(),
                    }
                    let hi = i.min(fa.len());
                    let lo = hi.saturating_sub(4);
                    if lo < hi {
                        out.push_str("  preceding context (matches on both sides):\n");
                        for f in &fa[lo..hi] {
                            out.push_str(&format!("    {} {} = {}\n", off(f.offset), f.path, clip(&f.value)));
                        }
                    }
                }
                None => out.push_str(
                    "\nstructural fields all match; the difference is inside an unparsed \
                     region — see the first differing byte above\n",
                ),
            }
        }
        (da, db) => {
            if let Err(e) = &da {
                out.push_str(&format!("\n(could not parse javac output: {e})\n"));
            }
            if let Err(e) = &db {
                out.push_str(&format!("(could not parse njavac output: {e})\n"));
            }
            if let Some(i) = first_differing_byte(a, b) {
                out.push_str(&format!("\nraw bytes around {}:\n", off(i)));
                out.push_str(&format!("  javac : {}\n", window(a, i)));
                out.push_str(&format!("  njavac: {}\n", window(b, i)));
            }
        }
    }
    Some(out)
}

/// Render a full structural dump as text, one field per line — the `classdiff
/// <file>` inspection view.
pub fn render_dump(fields: &[Field]) -> String {
    let mut out = String::new();
    for f in fields {
        out.push_str(&format!("{}  {} = {}\n", off(f.offset), f.path, clip(&f.value)));
    }
    out
}

fn field_differs(a: Option<&Field>, b: Option<&Field>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.path != y.path || x.value != y.value,
        (None, None) => false,
        _ => true,
    }
}

fn first_differing_byte(a: &[u8], b: &[u8]) -> Option<usize> {
    (0..a.len().min(b.len())).find(|&i| a[i] != b[i])
}

fn off(o: usize) -> String {
    format!("0x{o:X} ({o})")
}

/// A short hex window around byte `i`, for the raw-fallback report.
fn window(bytes: &[u8], i: usize) -> String {
    let lo = i.saturating_sub(4);
    let hi = (i + 5).min(bytes.len());
    raw(&bytes[lo..hi])
}

/// Truncate a long rendered value for display (the full value is still what the
/// diff compares — only the report line is clipped).
fn clip(s: &str) -> String {
    const MAX: usize = 72;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let head: String = s.chars().take(MAX).collect();
        format!("{head}… ({} chars)", s.chars().count())
    }
}
