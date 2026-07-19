use std::collections::HashMap;

/// One decoded field: where it starts in the file, its structural path, and its
/// value rendered as text. Two byte-identical class files produce identical field
/// lists; the first place the lists differ is the first structural divergence.
pub struct Field {
    pub offset: usize,
    pub path: String,
    pub value: String,
}

/// Parse a `.class` byte stream into its ordered structural fields. Returns an
/// error string if the bytes are not a parseable class file (truncated, bad
/// magic, an unknown constant-pool tag) — the caller falls back to a raw byte
/// diff in that case.
pub fn dump(bytes: &[u8]) -> Result<Vec<Field>, String> {
    let mut r = Reader {
        b: bytes,
        pos: 0,
        fields: Vec::new(),
        utf8: HashMap::new(),
    };
    r.parse()?;
    if r.pos != bytes.len() {
        let off = r.pos;
        r.fields.push(Field {
            offset: off,
            path: "<trailing>".into(),
            value: raw(&bytes[off..]),
        });
    }
    Ok(r.fields)
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    fields: Vec<Field>,
    /// index -> decoded Utf8 value, for resolving attribute names and readable hints.
    utf8: HashMap<u16, String>,
}

impl<'a> Reader<'a> {
    fn parse(&mut self) -> Result<(), String> {
        let m_off = self.pos;
        let magic = self.ru32()?;
        self.fields.push(Field {
            offset: m_off,
            path: "magic".into(),
            value: format!("0x{magic:08X}"),
        });
        if magic != 0xCAFE_BABE {
            return Err(format!("bad magic 0x{magic:08X}"));
        }
        self.fu16("minor_version")?;
        self.fu16("major_version")?;
        self.constant_pool()?;
        self.fflags("access_flags")?;
        self.fu16("this_class")?;
        self.fu16("super_class")?;
        let ic = self.fu16("interfaces_count")?;
        for i in 0..ic {
            self.fu16(format!("interfaces[{i}]"))?;
        }
        let fc = self.fu16("fields_count")?;
        for i in 0..fc {
            self.member("fields", i)?;
        }
        let mc = self.fu16("methods_count")?;
        for i in 0..mc {
            self.member("methods", i)?;
        }
        self.attributes("class")?;
        Ok(())
    }

    fn constant_pool(&mut self) -> Result<(), String> {
        let count = self.fu16("constant_pool_count")?;
        let mut i = 1u16;
        while i < count {
            let tag_off = self.pos;
            let tag = self.ru8()?;
            self.fields.push(Field {
                offset: tag_off,
                path: format!("cp[{i}].tag"),
                value: format!("{tag} ({})", cp_tag_name(tag)),
            });
            match tag {
                1 => {
                    // Utf8: u2 length, then bytes.
                    let len = self.fu16(format!("cp[{i}].length"))?;
                    let s_off = self.pos;
                    let bytes = self.rbytes(len as usize)?;
                    let text = String::from_utf8_lossy(bytes).into_owned();
                    self.utf8.insert(i, text.clone());
                    self.fields.push(Field {
                        offset: s_off,
                        path: format!("cp[{i}].bytes"),
                        value: quote(&text),
                    });
                }
                3 => {
                    let o = self.pos;
                    let v = self.ru32()?;
                    self.fields.push(Field {
                        offset: o,
                        path: format!("cp[{i}].int"),
                        value: (v as i32).to_string(),
                    });
                }
                4 => {
                    let o = self.pos;
                    let v = self.ru32()?;
                    self.fields.push(Field {
                        offset: o,
                        path: format!("cp[{i}].float_bits"),
                        value: format!("0x{v:08X}"),
                    });
                }
                5 => {
                    self.fu32(format!("cp[{i}].long_hi"))?;
                    self.fu32(format!("cp[{i}].long_lo"))?;
                }
                6 => {
                    let o = self.pos;
                    let hi = self.ru32()?;
                    self.fields.push(Field {
                        offset: o,
                        path: format!("cp[{i}].double_hi"),
                        value: format!("0x{hi:08X}"),
                    });
                    let o = self.pos;
                    let lo = self.ru32()?;
                    self.fields.push(Field {
                        offset: o,
                        path: format!("cp[{i}].double_lo"),
                        value: format!("0x{lo:08X}"),
                    });
                }
                7 => {
                    self.fu16_utf8(format!("cp[{i}].Class.name_index"))?;
                }
                8 => {
                    self.fu16_utf8(format!("cp[{i}].String.utf8_index"))?;
                }
                9 => {
                    self.fu16(format!("cp[{i}].Fieldref.class_index"))?;
                    self.fu16(format!("cp[{i}].Fieldref.nat_index"))?;
                }
                10 => {
                    self.fu16(format!("cp[{i}].Methodref.class_index"))?;
                    self.fu16(format!("cp[{i}].Methodref.nat_index"))?;
                }
                11 => {
                    self.fu16(format!("cp[{i}].InterfaceMethodref.class_index"))?;
                    self.fu16(format!("cp[{i}].InterfaceMethodref.nat_index"))?;
                }
                12 => {
                    self.fu16_utf8(format!("cp[{i}].NameAndType.name_index"))?;
                    self.fu16_utf8(format!("cp[{i}].NameAndType.desc_index"))?;
                }
                15 => {
                    self.fu8(format!("cp[{i}].MethodHandle.reference_kind"))?;
                    self.fu16(format!("cp[{i}].MethodHandle.reference_index"))?;
                }
                16 => {
                    self.fu16_utf8(format!("cp[{i}].MethodType.desc_index"))?;
                }
                17 => {
                    self.fu16(format!("cp[{i}].Dynamic.bsm_attr_index"))?;
                    self.fu16(format!("cp[{i}].Dynamic.nat_index"))?;
                }
                18 => {
                    self.fu16(format!("cp[{i}].InvokeDynamic.bsm_attr_index"))?;
                    self.fu16(format!("cp[{i}].InvokeDynamic.nat_index"))?;
                }
                19 => {
                    self.fu16_utf8(format!("cp[{i}].Module.name_index"))?;
                }
                20 => {
                    self.fu16_utf8(format!("cp[{i}].Package.name_index"))?;
                }
                _ => {
                    return Err(format!(
                        "unknown constant-pool tag {tag} at cp[{i}] (offset {tag_off})"
                    ));
                }
            }
            // Long and Double each consume two pool indices (JVMS 4.4.5).
            i += if tag == 5 || tag == 6 { 2 } else { 1 };
        }
        Ok(())
    }

    fn member(&mut self, kind: &str, i: u16) -> Result<(), String> {
        let p = format!("{kind}[{i}]");
        self.fflags(format!("{p}.access_flags"))?;
        self.fu16_utf8(format!("{p}.name_index"))?;
        self.fu16_utf8(format!("{p}.descriptor_index"))?;
        self.attributes(&p)?;
        Ok(())
    }

    fn attributes(&mut self, owner: &str) -> Result<(), String> {
        let count = self.fu16(format!("{owner}.attributes_count"))?;
        for a in 0..count {
            let name_index = self.fu16_utf8(format!("{owner}.attr[{a}].name_index"))?;
            let len = self.fu32(format!("{owner}.attr[{a}].length"))?;
            let name = self.utf8.get(&name_index).cloned().unwrap_or_default();
            let end = self.pos + len as usize;
            let ap = format!(
                "{owner}.attr[{a}].{}",
                if name.is_empty() { "?" } else { &name }
            );
            match name.as_str() {
                "Code" => self.code_attr(&ap)?,
                "LineNumberTable" => self.line_number_table(&ap)?,
                "StackMapTable" => self.stack_map_table(&ap)?,
                "SourceFile" => {
                    self.fu16_utf8(format!("{ap}.sourcefile_index"))?;
                }
                _ => {
                    self.fraw(format!("{ap}.info"), len as usize)?;
                }
            }
            // Resync to the declared length: if a structural decode consumed fewer
            // bytes than the attribute claims (a shape we don't fully model), soak
            // up the rest so the parse stays aligned; overrun is a real error.
            if self.pos < end {
                self.fraw(format!("{ap}.<unparsed-tail>"), end - self.pos)?;
            } else if self.pos > end {
                return Err(format!(
                    "attribute {ap} overran its declared length ({len})"
                ));
            }
        }
        Ok(())
    }

    fn code_attr(&mut self, ap: &str) -> Result<(), String> {
        self.fu16(format!("{ap}.max_stack"))?;
        self.fu16(format!("{ap}.max_locals"))?;
        let code_len = self.fu32(format!("{ap}.code_length"))?;
        self.fraw(format!("{ap}.code"), code_len as usize)?;
        let etl = self.fu16(format!("{ap}.exception_table_length"))?;
        for e in 0..etl {
            self.fu16(format!("{ap}.exc[{e}].start_pc"))?;
            self.fu16(format!("{ap}.exc[{e}].end_pc"))?;
            self.fu16(format!("{ap}.exc[{e}].handler_pc"))?;
            self.fu16(format!("{ap}.exc[{e}].catch_type"))?;
        }
        self.attributes(ap)?;
        Ok(())
    }

    fn line_number_table(&mut self, ap: &str) -> Result<(), String> {
        let n = self.fu16(format!("{ap}.table_length"))?;
        for e in 0..n {
            self.fu16(format!("{ap}.line[{e}].start_pc"))?;
            self.fu16(format!("{ap}.line[{e}].line_number"))?;
        }
        Ok(())
    }

    fn stack_map_table(&mut self, ap: &str) -> Result<(), String> {
        let n = self.fu16(format!("{ap}.number_of_entries"))?;
        for e in 0..n {
            let ft_off = self.pos;
            let ft = self.ru8()?;
            self.fields.push(Field {
                offset: ft_off,
                path: format!("{ap}.frame[{e}].type"),
                value: format!("{ft} ({})", frame_kind(ft)),
            });
            let fp = format!("{ap}.frame[{e}]");
            match ft {
                0..=63 => {} // same_frame
                64..=127 => self.verification_type(&format!("{fp}.stack[0]"))?,
                247 => {
                    self.fu16(format!("{fp}.offset_delta"))?;
                    self.verification_type(&format!("{fp}.stack[0]"))?;
                }
                248..=251 => {
                    self.fu16(format!("{fp}.offset_delta"))?;
                }
                252..=254 => {
                    self.fu16(format!("{fp}.offset_delta"))?;
                    for l in 0..(ft - 251) {
                        self.verification_type(&format!("{fp}.locals[{l}]"))?;
                    }
                }
                255 => {
                    self.fu16(format!("{fp}.offset_delta"))?;
                    let nl = self.fu16(format!("{fp}.number_of_locals"))?;
                    for l in 0..nl {
                        self.verification_type(&format!("{fp}.locals[{l}]"))?;
                    }
                    let ns = self.fu16(format!("{fp}.number_of_stack_items"))?;
                    for s in 0..ns {
                        self.verification_type(&format!("{fp}.stack[{s}]"))?;
                    }
                }
                // 128..=246 are reserved with no defined layout; bail so the caller
                // falls back to a raw byte diff rather than desync the parse.
                128..=246 => return Err(format!("reserved stack-map frame type {ft} at {fp}")),
            }
        }
        Ok(())
    }

    fn verification_type(&mut self, path: &str) -> Result<(), String> {
        let o = self.pos;
        let tag = self.ru8()?;
        self.fields.push(Field {
            offset: o,
            path: format!("{path}.tag"),
            value: format!("{tag} ({})", vti_name(tag)),
        });
        // Object (7) carries a cpool Class index; Uninitialized (8) an offset.
        if tag == 7 {
            self.fu16(format!("{path}.cpool_index"))?;
        } else if tag == 8 {
            self.fu16(format!("{path}.offset"))?;
        }
        Ok(())
    }

    // ---- field-recording primitives ----

    fn fu8(&mut self, path: impl Into<String>) -> Result<u8, String> {
        let o = self.pos;
        let v = self.ru8()?;
        self.fields.push(Field {
            offset: o,
            path: path.into(),
            value: v.to_string(),
        });
        Ok(v)
    }

    fn fu16(&mut self, path: impl Into<String>) -> Result<u16, String> {
        let o = self.pos;
        let v = self.ru16()?;
        self.fields.push(Field {
            offset: o,
            path: path.into(),
            value: v.to_string(),
        });
        Ok(v)
    }

    fn fu32(&mut self, path: impl Into<String>) -> Result<u32, String> {
        let o = self.pos;
        let v = self.ru32()?;
        self.fields.push(Field {
            offset: o,
            path: path.into(),
            value: v.to_string(),
        });
        Ok(v)
    }

    /// A u16 rendered in hex (access-flag bitfields).
    fn fflags(&mut self, path: impl Into<String>) -> Result<u16, String> {
        let o = self.pos;
        let v = self.ru16()?;
        self.fields.push(Field {
            offset: o,
            path: path.into(),
            value: format!("0x{v:04X}"),
        });
        Ok(v)
    }

    /// A u16 constant-pool index, annotated with the referent Utf8 when the index
    /// resolves to one already parsed (the readable hint never hides a real diff —
    /// the raw index is part of the value and is compared).
    fn fu16_utf8(&mut self, path: impl Into<String>) -> Result<u16, String> {
        let o = self.pos;
        let v = self.ru16()?;
        let value = match self.utf8.get(&v) {
            Some(s) => format!("{v} -> {}", quote(s)),
            None => v.to_string(),
        };
        self.fields.push(Field {
            offset: o,
            path: path.into(),
            value,
        });
        Ok(v)
    }

    fn fraw(&mut self, path: impl Into<String>, n: usize) -> Result<(), String> {
        let o = self.pos;
        let bytes = self.rbytes(n)?;
        self.fields.push(Field {
            offset: o,
            path: path.into(),
            value: format!("[{n} bytes] {}", raw(bytes)),
        });
        Ok(())
    }

    // ---- raw readers ----

    fn need(&self, n: usize) -> Result<(), String> {
        if self.pos + n > self.b.len() {
            Err(format!(
                "unexpected EOF at offset {} (need {n} more bytes, have {})",
                self.pos,
                self.b.len() - self.pos
            ))
        } else {
            Ok(())
        }
    }

    fn ru8(&mut self) -> Result<u8, String> {
        self.need(1)?;
        let v = self.b[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn ru16(&mut self) -> Result<u16, String> {
        self.need(2)?;
        let v = u16::from_be_bytes([self.b[self.pos], self.b[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn ru32(&mut self) -> Result<u32, String> {
        self.need(4)?;
        let v = u32::from_be_bytes([
            self.b[self.pos],
            self.b[self.pos + 1],
            self.b[self.pos + 2],
            self.b[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn rbytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        self.need(n)?;
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

pub(super) fn raw(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn cp_tag_name(tag: u8) -> &'static str {
    match tag {
        1 => "Utf8",
        3 => "Integer",
        4 => "Float",
        5 => "Long",
        6 => "Double",
        7 => "Class",
        8 => "String",
        9 => "Fieldref",
        10 => "Methodref",
        11 => "InterfaceMethodref",
        12 => "NameAndType",
        15 => "MethodHandle",
        16 => "MethodType",
        17 => "Dynamic",
        18 => "InvokeDynamic",
        19 => "Module",
        20 => "Package",
        _ => "?",
    }
}

fn frame_kind(ft: u8) -> &'static str {
    match ft {
        0..=63 => "same",
        64..=127 => "same_locals_1_stack_item",
        247 => "same_locals_1_stack_item_extended",
        248..=250 => "chop",
        251 => "same_frame_extended",
        252..=254 => "append",
        255 => "full",
        _ => "reserved",
    }
}

fn vti_name(tag: u8) -> &'static str {
    match tag {
        0 => "Top",
        1 => "Integer",
        2 => "Float",
        3 => "Double",
        4 => "Long",
        5 => "Null",
        6 => "UninitializedThis",
        7 => "Object",
        8 => "Uninitialized",
        _ => "?",
    }
}
