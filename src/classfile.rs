//! Class file model + serializer.
//!
//! The tricky part of matching `javac` byte-for-byte is the constant pool:
//! entries must appear in the *exact* order javac emits them. Empirically
//! (see reference/HelloWorld dump) javac interns each composite entry
//! breadth-first: a Methodref takes its own slot, then its Class and
//! NameAndType take slots, then *their* Utf8 children, and so on. We
//! reproduce that with a FIFO queue per top-level intern call.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::fxhash::FxHashMap;

mod modified_utf8;

/// A pool-local identity for one deduplicated string. Composite entries use these
/// integer identities so their keys never re-hash or compare string contents.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TextId(u16);

/// A logical constant-pool entry. Child references use pool-local text identities
/// and are resolved to constant-pool slots at serialization time via `index`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Entry {
    Utf8(TextId),
    /// A CONSTANT_Integer: a 4-byte `int` value. A leaf (no children).
    Integer(i32),
    /// A CONSTANT_Long: an 8-byte `long`. A leaf; **occupies two pool indices**.
    Long(i64),
    /// A CONSTANT_Float, stored as its 32-bit pattern *after NaN canonicalization*
    /// (see `ConstantPool::float`): every NaN collapses to `0x7fc00000`, but `-0.0f`
    /// stays a distinct entry from `+0.0f`, matching `Float.floatToIntBits`. A leaf.
    Float(u32),
    /// A CONSTANT_Double, same NaN-canonicalized bit rule (`0x7ff8000000000000`),
    /// per `Double.doubleToLongBits`. A leaf; **occupies two pool indices**.
    Double(u64),
    /// Class by internal name, e.g. "java/lang/Object". Child: Utf8(name).
    Class(TextId),
    /// name + descriptor. Children: Utf8(name), Utf8(desc).
    NameAndType(TextId, TextId),
    /// owner + name + descriptor. Children: Class(owner), NameAndType(name, desc).
    Fieldref(TextId, TextId, TextId),
    Methodref(TextId, TextId, TextId),
    /// String constant. Child: Utf8(value).
    StringConst(TextId),
}

impl Entry {
    /// Direct children in the order javac enqueues them.
    fn children(self) -> [Option<Entry>; 2] {
        match self {
            Entry::Utf8(_)
            | Entry::Integer(_)
            | Entry::Long(_)
            | Entry::Float(_)
            | Entry::Double(_) => [None, None],
            Entry::Class(n) => [Some(Entry::Utf8(n)), None],
            Entry::NameAndType(n, d) =>
                [Some(Entry::Utf8(n)), Some(Entry::Utf8(d))],
            Entry::Fieldref(o, n, d) | Entry::Methodref(o, n, d) => [
                Some(Entry::Class(o)),
                Some(Entry::NameAndType(n, d)),
            ],
            Entry::StringConst(s) => [Some(Entry::Utf8(s)), None],
        }
    }

    /// Number of constant-pool indices this entry consumes. `Long`/`Double` take
    /// two (the second index is an unusable phantom), per JVMS 4.4.5; everything
    /// else takes one.
    fn width(&self) -> u16 {
        match self {
            Entry::Long(_) | Entry::Double(_) => 2,
            _ => 1,
        }
    }
}

pub struct ConstantPool {
    /// Text storage is independent of pool-entry order: `TextId` is only an
    /// internal key, while `entries` remains the sole serialization order.
    texts: Vec<Rc<str>>,
    text_index: FxHashMap<Rc<str>, TextId>,
    entries: Vec<Entry>,
    index: FxHashMap<Entry, u16>,
    /// Reused scratch storage for one breadth-first intern walk.
    queue: VecDeque<Entry>,
    /// Index the next interned entry will receive (also the `constant_pool_count`).
    next_index: u16,
}

impl ConstantPool {
    pub fn new() -> Self {
        // Presize for a typical class's pool (~15-40 entries) so interning does not
        // repeatedly reallocate as entries accumulate.
        const CAP: usize = 48;
        ConstantPool {
            texts: Vec::with_capacity(CAP),
            text_index: FxHashMap::with_capacity_and_hasher(CAP, Default::default()),
            entries: Vec::with_capacity(CAP),
            index: FxHashMap::with_capacity_and_hasher(CAP, Default::default()),
            queue: VecDeque::with_capacity(4),
            next_index: 1,
        }
    }

    /// The `constant_pool_count` field: one past the last assigned index, which
    /// already accounts for every phantom `Long`/`Double` slot.
    pub fn count(&self) -> u16 {
        self.next_index
    }

    /// Append a single entry (no child handling), assigning it the next index and
    /// advancing the counter by the entry's width.
    fn alloc_new(&mut self, e: Entry) -> u16 {
        debug_assert!(!self.index.contains_key(&e));
        let idx = self.next_index;
        self.next_index += e.width();
        self.entries.push(e);
        self.index.insert(e, idx);
        idx
    }

    /// Intern an entry and, breadth-first, all of its not-yet-present children,
    /// reproducing javac's emission order. Returns the entry's slot.
    fn intern(&mut self, e: Entry) -> u16 {
        if let Some(&i) = self.index.get(&e) {
            return i;
        }
        debug_assert!(self.queue.is_empty());
        self.queue.push_back(e);
        let root = self.alloc_new(e);
        while let Some(cur) = self.queue.pop_front() {
            for child in cur.children().into_iter().flatten() {
                if !self.index.contains_key(&child) {
                    self.queue.push_back(child);
                    self.alloc_new(child);
                }
            }
        }
        root
    }

    /// Deduplicate text once at the constant-pool boundary. Assigning a `TextId`
    /// has no byte-level effect; only the ordered `Entry::Utf8` insertion does.
    fn text(&mut self, value: &str) -> TextId {
        if let Some(&id) = self.text_index.get(value) {
            return id;
        }
        let id = TextId(self.texts.len() as u16);
        let value: Rc<str> = Rc::from(value);
        self.texts.push(value.clone());
        self.text_index.insert(value, id);
        id
    }

    fn text_id(&self, value: &str) -> TextId {
        *self
            .text_index
            .get(value)
            .unwrap_or_else(|| panic!("text not interned: {value}"))
    }

    fn entry_index(&self, entry: Entry) -> u16 {
        *self.index.get(&entry).expect("constant-pool entry not interned")
    }

    // Public interning API, one method per operand kind.
    pub fn utf8(&mut self, s: &str) -> u16 {
        let s = self.text(s);
        self.intern(Entry::Utf8(s))
    }
    pub fn integer(&mut self, v: i32) -> u16 {
        self.intern(Entry::Integer(v))
    }
    pub fn long(&mut self, v: i64) -> u16 {
        self.intern(Entry::Long(v))
    }
    pub fn float(&mut self, v: f32) -> u16 {
        // javac writes float constants through `Float.floatToIntBits`, which
        // canonicalizes *every* NaN to `0x7fc00000` (a folded `-(0.0f/0.0f)` keeps a
        // sign-flipped NaN otherwise). `-0.0f` is not a NaN, so it stays distinct.
        let bits = if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() };
        self.intern(Entry::Float(bits))
    }
    pub fn double(&mut self, v: f64) -> u16 {
        // Same canonicalization via `Double.doubleToLongBits` (`0x7ff8000000000000`).
        let bits = if v.is_nan() { 0x7ff8_0000_0000_0000 } else { v.to_bits() };
        self.intern(Entry::Double(bits))
    }
    pub fn class(&mut self, internal_name: &str) -> u16 {
        let internal_name = self.text(internal_name);
        self.intern(Entry::Class(internal_name))
    }
    pub fn string(&mut self, s: &str) -> u16 {
        let s = self.text(s);
        self.intern(Entry::StringConst(s))
    }
    pub fn fieldref(&mut self, owner: &str, name: &str, desc: &str) -> u16 {
        let owner = self.text(owner);
        let name = self.text(name);
        let desc = self.text(desc);
        self.intern(Entry::Fieldref(owner, name, desc))
    }
    pub fn methodref(&mut self, owner: &str, name: &str, desc: &str) -> u16 {
        let owner = self.text(owner);
        let name = self.text(name);
        let desc = self.text(desc);
        self.intern(Entry::Methodref(owner, name, desc))
    }

    /// The slot of an already-interned `Class`, for resolving a StackMapTable
    /// `Object` verification type. Panics if the class was never interned — a
    /// frame must not reference a class codegen did not put in the pool.
    pub fn class_index(&self, internal_name: &str) -> u16 {
        let name = self.text_id(internal_name);
        self.entry_index(Entry::Class(name))
    }

    /// The slot of an already-interned `Utf8`. Attribute writing uses this only
    /// after the phase-2 interning walk has frozen the pool.
    pub fn utf8_index(&self, value: &str) -> u16 {
        let value = self.text_id(value);
        self.entry_index(Entry::Utf8(value))
    }

    fn serialize(&self, buf: &mut ByteBuf) {
        buf.u16(self.count());
        for e in &self.entries {
            match e {
                Entry::Utf8(s) => {
                    buf.u8(1);
                    modified_utf8::write(&self.texts[s.0 as usize], buf);
                }
                Entry::Integer(v) => {
                    buf.u8(3);
                    buf.u32(*v as u32);
                }
                Entry::Float(bits) => {
                    buf.u8(4);
                    buf.u32(*bits);
                }
                Entry::Long(v) => {
                    buf.u8(5);
                    buf.u32((*v as u64 >> 32) as u32);
                    buf.u32(*v as u64 as u32);
                }
                Entry::Double(bits) => {
                    buf.u8(6);
                    buf.u32((*bits >> 32) as u32);
                    buf.u32(*bits as u32);
                }
                Entry::Class(n) => {
                    buf.u8(7);
                    buf.u16(self.entry_index(Entry::Utf8(*n)));
                }
                Entry::NameAndType(n, d) => {
                    buf.u8(12);
                    buf.u16(self.entry_index(Entry::Utf8(*n)));
                    buf.u16(self.entry_index(Entry::Utf8(*d)));
                }
                Entry::Fieldref(o, n, d) => {
                    buf.u8(9);
                    buf.u16(self.entry_index(Entry::Class(*o)));
                    buf.u16(self.entry_index(Entry::NameAndType(*n, *d)));
                }
                Entry::Methodref(o, n, d) => {
                    buf.u8(10);
                    buf.u16(self.entry_index(Entry::Class(*o)));
                    buf.u16(self.entry_index(Entry::NameAndType(*n, *d)));
                }
                Entry::StringConst(s) => {
                    buf.u8(8);
                    buf.u16(self.entry_index(Entry::Utf8(*s)));
                }
            }
        }
    }
}

/// A `verification_type_info` in a StackMapTable frame — the verifier's view of
/// one local or stack slot. The four small integral types
/// (`boolean`/`byte`/`char`/`short`/`int`) all map to `Integer`; `Top` preserves an
/// interior physical slot for an uninitialized local. `Object` carries the
/// referenced class's internal name, resolved to its constant-pool `Class` index
/// when the frame is serialized. A `Long`/`Double` is a **single** entry even
/// though it occupies two JVM slots.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VerificationType {
    Top,          // tag 0
    Integer,      // tag 1
    Float,        // tag 2
    Double,       // tag 3
    Long,         // tag 4
    Object(String), // tag 7 + u2 Class index
}

impl VerificationType {
    fn write(&self, buf: &mut ByteBuf, cp: &ConstantPool) {
        match self {
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
}

/// One stack-map point: the full verifier state (locals + operand stack) at a
/// branch target, keyed by its absolute bytecode offset. Codegen produces these
/// as complete snapshots in increasing-offset order; the serializer derives each
/// frame's `offset_delta` (with javac's −1 inter-frame bias) and picks the
/// smallest frame form relative to the previous frame's state.
pub struct StackFrame {
    pub offset: u16,
    pub locals: Vec<VerificationType>,
    pub stack: Vec<VerificationType>,
}

/// One owned class-file attribute. Vector order is serialization order and also
/// drives phase-2 constant interning.
pub enum Attribute {
    Code(CodeAttribute),
    LineNumberTable(Vec<(u16, u16)>),
    StackMapTable {
        entry_locals: Vec<VerificationType>,
        frames: Vec<StackFrame>,
    },
    SourceFile(String),
}

impl Attribute {
    fn name(&self) -> &'static str {
        match self {
            Attribute::Code(_) => "Code",
            Attribute::LineNumberTable(_) => "LineNumberTable",
            Attribute::StackMapTable { .. } => "StackMapTable",
            Attribute::SourceFile(_) => "SourceFile",
        }
    }
}

/// The body of a `Code` attribute. Exception handlers are not supported yet, so
/// `exception_table` records the only current state without modeling future rows.
pub struct CodeAttribute {
    pub max_stack: u16,
    pub max_locals: u16,
    pub code: Vec<u8>,
    pub exception_table: (),
    pub attributes: Vec<Attribute>,
}

impl CodeAttribute {
    pub fn new(
        max_stack: u16,
        max_locals: u16,
        code: Vec<u8>,
        line_numbers: Vec<(u16, u16)>,
        entry_locals: Vec<VerificationType>,
        stack_frames: Vec<StackFrame>,
    ) -> Self {
        let mut attributes = vec![Attribute::LineNumberTable(line_numbers)];
        if !stack_frames.is_empty() {
            attributes.push(Attribute::StackMapTable {
                entry_locals,
                frames: stack_frames,
            });
        }
        CodeAttribute {
            max_stack,
            max_locals,
            code,
            exception_table: (),
            attributes,
        }
    }
}

/// One method with its attributes in class-file order.
pub struct Method {
    pub access_flags: u16,
    pub name: String,
    pub descriptor: String,
    pub attributes: Vec<Attribute>,
}

impl Method {
    pub fn with_code(
        access_flags: u16,
        name: impl Into<String>,
        descriptor: impl Into<String>,
        code: CodeAttribute,
    ) -> Self {
        Method {
            access_flags,
            name: name.into(),
            descriptor: descriptor.into(),
            attributes: vec![Attribute::Code(code)],
        }
    }
}

pub struct ClassFile {
    pub access_flags: u16,
    pub this_class: String,
    pub super_class: String,
    pub methods: Vec<Method>,
    pub attributes: Vec<Attribute>,
}

impl ClassFile {
    pub fn new(
        access_flags: u16,
        this_class: impl Into<String>,
        super_class: impl Into<String>,
        methods: Vec<Method>,
        source_file: impl Into<String>,
    ) -> Self {
        ClassFile {
            access_flags,
            this_class: this_class.into(),
            super_class: super_class.into(),
            methods,
            attributes: vec![Attribute::SourceFile(source_file.into())],
        }
    }

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

/// Intern each attribute name followed by exactly the body constants and children
/// that writing the same recursive vectors will visit.
fn intern_attributes(attributes: &[Attribute], cp: &mut ConstantPool) {
    for attribute in attributes {
        cp.utf8(attribute.name());
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
        buf.u16(cp.utf8_index(attribute.name()));
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
                vt.write(buf, cp);
            }
            FrameShape::SameLocals1(vt) => {
                buf.u8(247); // same_locals_1_stack_item_frame_extended
                buf.u16(delta);
                vt.write(buf, cp);
            }
            FrameShape::Append(new) => {
                buf.u8(251 + new.len() as u8); // append_frame (k = 1..=3)
                buf.u16(delta);
                for vt in new {
                    vt.write(buf, cp);
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
                    vt.write(buf, cp);
                }
                buf.u16(f.stack.len() as u16);
                for vt in &f.stack {
                    vt.write(buf, cp);
                }
            }
        }

        prev_offset = Some(f.offset);
        prev_locals = &f.locals;
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

/// Big-endian byte buffer.
pub struct ByteBuf(Vec<u8>);
impl ByteBuf {
    pub fn with_capacity(n: usize) -> Self {
        ByteBuf(Vec::with_capacity(n))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub fn reserve_u32(&mut self) -> usize {
        let offset = self.len();
        self.u32(0);
        offset
    }
    pub fn patch_u32(&mut self, offset: usize, v: u32) {
        self.0[offset..offset + 4].copy_from_slice(&v.to_be_bytes());
    }
    pub fn bytes(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}
