use super::buffer::ByteBuf;
use super::model::{Attribute, ClassFile, StackFrame, VerificationType};
use super::pool::ConstantPool;

impl ClassFile {
    /// Serialize the whole file. `cp` must already contain the phase-1
    /// (code-generation) constants, interned in bytecode-reference order by the
    /// caller. Here we add the phase-2 (writing-order) structural constants and
    /// emit the bytes.
    pub fn to_bytes(&self, mut cp: ConstantPool) -> Vec<u8> {
        // Phase 2: writing order.
        let this_idx = cp.class(&self.this_class);
        let super_idx = cp.class(&self.super_class);

        // Per-method structural Utf8s and recursive attributes, in declaration
        // and serialization order.
        for m in &self.methods {
            cp.utf8(&m.name);
            cp.utf8(&m.descriptor);
            intern_attributes(&m.attributes, &mut cp);
        }
        intern_attributes(&self.attributes, &mut cp);

        // The pool is frozen after the phase-2 walk. Body builders and attribute
        // writers below may only resolve existing entries through immutable lookup.

        // ---- serialize ----
        // Presize to a whole small class file so the output never reallocs mid-write
        // (fixtures average ~500 bytes; this covers the vast majority in one alloc).
        let mut buf = ByteBuf::with_capacity(1024);
        buf.u32(0xCAFEBABE);
        buf.u16(0); // minor
        buf.u16(69); // major: Java 25
        cp.serialize(&mut buf);
        buf.u16(self.access_flags);
        buf.u16(this_idx);
        buf.u16(super_idx);
        buf.u16(0); // interfaces_count
        buf.u16(0); // fields_count

        buf.u16(self.methods.len() as u16);
        for m in &self.methods {
            buf.u16(m.access_flags);
            buf.u16(cp.utf8_index(&m.name));
            buf.u16(cp.utf8_index(&m.descriptor));
            write_attributes(&mut buf, &m.attributes, &cp);
        }

        write_attributes(&mut buf, &self.attributes, &cp);

        buf.into_vec()
    }
}

fn attribute_name(attribute: &Attribute) -> &'static str {
    match attribute {
        Attribute::Code(_) => "Code",
        Attribute::LineNumberTable(_) => "LineNumberTable",
        Attribute::StackMapTable { .. } => "StackMapTable",
        Attribute::SourceFile(_) => "SourceFile",
    }
}

/// Intern each attribute name followed by exactly the body constants and children
/// that writing the same recursive vectors will visit.
fn intern_attributes(attributes: &[Attribute], cp: &mut ConstantPool) {
    for attribute in attributes {
        cp.utf8(attribute_name(attribute));
        match attribute {
            Attribute::Code(code) => intern_attributes(&code.attributes, cp),
            Attribute::LineNumberTable(_) => {}
            Attribute::StackMapTable {
                entry_locals,
                frames,
            } => {
                // Only Object classes present in the selected serialized frame
                // shapes enter the pool; full snapshots are not the write plan.
                for name in frame_object_classes(frames, entry_locals) {
                    cp.class(&name);
                }
            }
            Attribute::SourceFile(source_file) => {
                cp.utf8(source_file);
            }
        }
    }
}

/// Write an ordered attribute vector directly into the class buffer. Each body
/// reserves its length field, writes recursively, then backpatches the measured
/// byte count; no parallel size model or temporary body buffer can drift.
fn write_attributes(buf: &mut ByteBuf, attributes: &[Attribute], cp: &ConstantPool) {
    buf.u16(attributes.len() as u16);
    for attribute in attributes {
        buf.u16(cp.utf8_index(attribute_name(attribute)));
        let length_at = buf.reserve_u32();
        let body_start = buf.len();
        write_attribute_body(buf, attribute, cp);
        buf.patch_u32(length_at, (buf.len() - body_start) as u32);
    }
}

fn write_attribute_body(buf: &mut ByteBuf, attribute: &Attribute, cp: &ConstantPool) {
    match attribute {
        Attribute::Code(code) => {
            buf.u16(code.max_stack);
            buf.u16(code.max_locals);
            buf.u32(code.code.len() as u32);
            buf.bytes(&code.code);
            buf.u16(0); // exception_table_length
            write_attributes(buf, &code.attributes, cp);
        }
        Attribute::LineNumberTable(line_numbers) => {
            buf.u16(line_numbers.len() as u16);
            for &(pc, line) in line_numbers {
                buf.u16(pc);
                buf.u16(line);
            }
        }
        Attribute::StackMapTable {
            entry_locals,
            frames,
        } => {
            write_stack_map_body(buf, frames, entry_locals, cp);
        }
        Attribute::SourceFile(source_file) => buf.u16(cp.utf8_index(source_file)),
    }
}

/// Write a method's StackMapTable attribute body (`number_of_entries` followed by
/// the frames) into the caller's measured attribute-body buffer.
///
/// For each frame the `offset_delta` uses javac's rule — the first frame's delta
/// is its absolute offset; every later frame's is `offset − prevOffset − 1` (the
/// −1 inter-frame bias) — and the smallest frame form that expresses the change
/// from the previous frame's state is chosen (`same` / `same_locals_1_stack_item`
/// / `append` / `chop`, falling back to `full_frame`), exactly as javac does.
fn write_stack_map_body(
    buf: &mut ByteBuf,
    frames: &[StackFrame],
    entry_locals: &[VerificationType],
    cp: &ConstantPool,
) {
    buf.u16(frames.len() as u16);

    let mut prev_offset: Option<u16> = None;
    let mut prev_locals: &[VerificationType] = entry_locals;
    for f in frames {
        let delta = match prev_offset {
            None => f.offset,
            Some(p) => f.offset - p - 1,
        };
        match classify_frame(&f.locals, &f.stack, prev_locals) {
            FrameShape::Same if delta <= 63 => buf.u8(delta as u8),
            FrameShape::Same => {
                buf.u8(251); // same_frame_extended
                buf.u16(delta);
            }
            FrameShape::SameLocals1(vt) if delta <= 63 => {
                buf.u8(64 + delta as u8); // same_locals_1_stack_item_frame
                write_verification_type(vt, buf, cp);
            }
            FrameShape::SameLocals1(vt) => {
                buf.u8(247); // same_locals_1_stack_item_frame_extended
                buf.u16(delta);
                write_verification_type(vt, buf, cp);
            }
            FrameShape::Append(new) => {
                buf.u8(251 + new.len() as u8); // append_frame (k = 1..=3)
                buf.u16(delta);
                for vt in new {
                    write_verification_type(vt, buf, cp);
                }
            }
            FrameShape::Chop(k) => {
                buf.u8(251 - k); // chop_frame
                buf.u16(delta);
            }
            FrameShape::Full => {
                buf.u8(255);
                buf.u16(delta);
                buf.u16(f.locals.len() as u16);
                for vt in &f.locals {
                    write_verification_type(vt, buf, cp);
                }
                buf.u16(f.stack.len() as u16);
                for vt in &f.stack {
                    write_verification_type(vt, buf, cp);
                }
            }
        }

        prev_offset = Some(f.offset);
        prev_locals = &f.locals;
    }
}

fn write_verification_type(vt: &VerificationType, buf: &mut ByteBuf, cp: &ConstantPool) {
    match vt {
        VerificationType::Top => buf.u8(0),
        VerificationType::Integer => buf.u8(1),
        VerificationType::Float => buf.u8(2),
        VerificationType::Double => buf.u8(3),
        VerificationType::Long => buf.u8(4),
        VerificationType::Object(name) => {
            buf.u8(7);
            buf.u16(cp.class_index(name));
        }
    }
}

/// The frame form javac would pick for the transition from `prev` locals to the
/// current `locals`/`stack`, ignoring the `offset_delta` (which only selects
/// between a form and its `_extended` variant). The serializer and the pool-
/// interning pass share this so they always agree on which frames are full.
enum FrameShape<'a> {
    Same,
    SameLocals1(&'a VerificationType),
    Append(&'a [VerificationType]),
    Chop(u8),
    Full,
}

fn classify_frame<'a>(
    locals: &'a [VerificationType],
    stack: &'a [VerificationType],
    prev: &[VerificationType],
) -> FrameShape<'a> {
    if locals == prev {
        match stack {
            [] => FrameShape::Same,
            [one] => FrameShape::SameLocals1(one),
            _ => FrameShape::Full,
        }
    } else if stack.is_empty() && is_prefix(prev, locals) && locals.len() - prev.len() <= 3 {
        FrameShape::Append(&locals[prev.len()..])
    } else if stack.is_empty() && is_prefix(locals, prev) && prev.len() - locals.len() <= 3 {
        FrameShape::Chop((prev.len() - locals.len()) as u8)
    } else {
        FrameShape::Full
    }
}

/// The internal names of every `Class` a method's frames will reference, in
/// serialization order — the `Object` verification types that survive into the
/// chosen frame encodings. Codegen leaves these classes for `to_bytes` to intern
/// at javac's pool position (right after `StackMapTable`), so an `Object` local
/// (e.g. `args`) only enters the pool when a `full_frame` actually names it.
fn frame_object_classes(frames: &[StackFrame], entry_locals: &[VerificationType]) -> Vec<String> {
    let mut names = Vec::new();
    let mut collect = |vt: &VerificationType| {
        if let VerificationType::Object(name) = vt {
            names.push(name.clone());
        }
    };
    let mut prev: &[VerificationType] = entry_locals;
    for f in frames {
        match classify_frame(&f.locals, &f.stack, prev) {
            FrameShape::Same | FrameShape::Chop(_) => {}
            FrameShape::SameLocals1(vt) => collect(vt),
            FrameShape::Append(new) => new.iter().for_each(&mut collect),
            FrameShape::Full => {
                f.locals.iter().for_each(&mut collect);
                f.stack.iter().for_each(&mut collect);
            }
        }
        prev = &f.locals;
    }
    names
}

/// Whether `short` is a (not-necessarily-proper) prefix of `long`.
fn is_prefix(short: &[VerificationType], long: &[VerificationType]) -> bool {
    short.len() <= long.len() && long[..short.len()] == *short
}
