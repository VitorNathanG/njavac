//! Class file model + serializer.
//!
//! The tricky part of matching `javac` byte-for-byte is the constant pool:
//! entries must appear in the *exact* order javac emits them. Empirically
//! (see reference/HelloWorld dump) javac interns each composite entry
//! breadth-first: a Methodref takes its own slot, then its Class and
//! NameAndType take slots, then *their* Utf8 children, and so on. We
//! reproduce that with a FIFO queue per top-level intern call.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::{BuildHasher, Hasher};

/// A fast, dependency-free FxHash-style hasher for the constant-pool dedup map.
/// The pool interns dozens of short `String` keys per class, and the default
/// SipHash dominated codegen time in profiling. FxHash is roughly an order of
/// magnitude cheaper for short keys; and since the class file depends on the
/// insertion ORDER of entries (a `Vec`), not on the hash, this changes nothing
/// about the emitted bytes.
#[derive(Default)]
struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(K);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let mut w = [0u8; 8];
            w.copy_from_slice(&bytes[..8]);
            self.add(u64::from_le_bytes(w));
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut w = [0u8; 8];
            w[..bytes.len()].copy_from_slice(bytes);
            self.add(u64::from_le_bytes(w));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

#[derive(Default, Clone)]
struct FxBuildHasher;

impl BuildHasher for FxBuildHasher {
    type Hasher = FxHasher;
    #[inline]
    fn build_hasher(&self) -> FxHasher {
        FxHasher::default()
    }
}

type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;

/// A logical constant-pool entry, keyed by its owned contents so we can dedup
/// (intern) identical entries. Child references are stored as keys and resolved
/// to indices at serialization time via the intern map.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Entry {
    Utf8(String),
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
    Class(String),
    /// name + descriptor. Children: Utf8(name), Utf8(desc).
    NameAndType(String, String),
    /// owner + name + descriptor. Children: Class(owner), NameAndType(name, desc).
    Fieldref(String, String, String),
    Methodref(String, String, String),
    /// String constant. Child: Utf8(value).
    StringConst(String),
}

impl Entry {
    /// Direct children in the order javac enqueues them.
    fn children(&self) -> Vec<Entry> {
        match self {
            Entry::Utf8(_)
            | Entry::Integer(_)
            | Entry::Long(_)
            | Entry::Float(_)
            | Entry::Double(_) => vec![],
            Entry::Class(n) => vec![Entry::Utf8(n.clone())],
            Entry::NameAndType(n, d) => vec![Entry::Utf8(n.clone()), Entry::Utf8(d.clone())],
            Entry::Fieldref(o, n, d) | Entry::Methodref(o, n, d) => vec![
                Entry::Class(o.clone()),
                Entry::NameAndType(n.clone(), d.clone()),
            ],
            Entry::StringConst(s) => vec![Entry::Utf8(s.clone())],
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
    entries: Vec<Entry>,
    /// The 1-based pool index assigned to `entries[i]`. Diverges from `i + 1`
    /// once any `Long`/`Double` (which each burn two indices) has been interned.
    slots: Vec<u16>,
    index: FxHashMap<Entry, u16>,
    /// Index the next interned entry will receive (also the `constant_pool_count`).
    next_index: u16,
}

impl ConstantPool {
    pub fn new() -> Self {
        ConstantPool {
            entries: Vec::new(),
            slots: Vec::new(),
            index: HashMap::default(),
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
    fn alloc(&mut self, e: Entry) -> u16 {
        if let Some(&i) = self.index.get(&e) {
            return i;
        }
        let idx = self.next_index;
        self.next_index += e.width();
        self.entries.push(e.clone());
        self.slots.push(idx);
        self.index.insert(e, idx);
        idx
    }

    /// Intern an entry and, breadth-first, all of its not-yet-present children,
    /// reproducing javac's emission order. Returns the entry's slot.
    fn intern(&mut self, e: Entry) -> u16 {
        if let Some(&i) = self.index.get(&e) {
            return i;
        }
        let root = self.alloc(e.clone());
        let mut queue = VecDeque::new();
        queue.push_back(e);
        while let Some(cur) = queue.pop_front() {
            for child in cur.children() {
                if !self.index.contains_key(&child) {
                    self.alloc(child.clone());
                    queue.push_back(child);
                }
            }
        }
        root
    }

    // Public interning API, one method per operand kind.
    pub fn utf8(&mut self, s: &str) -> u16 {
        self.intern(Entry::Utf8(s.to_string()))
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
        self.intern(Entry::Class(internal_name.to_string()))
    }
    pub fn string(&mut self, s: &str) -> u16 {
        self.intern(Entry::StringConst(s.to_string()))
    }
    pub fn fieldref(&mut self, owner: &str, name: &str, desc: &str) -> u16 {
        self.intern(Entry::Fieldref(owner.to_string(), name.to_string(), desc.to_string()))
    }
    pub fn methodref(&mut self, owner: &str, name: &str, desc: &str) -> u16 {
        self.intern(Entry::Methodref(owner.to_string(), name.to_string(), desc.to_string()))
    }

    /// The slot of an already-interned `Class`, for resolving a StackMapTable
    /// `Object` verification type. Panics if the class was never interned — a
    /// frame must not reference a class codegen did not put in the pool.
    pub fn class_index(&self, internal_name: &str) -> u16 {
        *self
            .index
            .get(&Entry::Class(internal_name.to_string()))
            .unwrap_or_else(|| panic!("class not interned: {internal_name}"))
    }

    fn serialize(&self, buf: &mut ByteBuf) {
        // Resolve child indices through borrowed lookup tables built once from
        // the ordered entries, so writing never reconstructs or clones an `Entry`
        // key. Each table maps the child content a composite entry references to
        // that child's slot.
        let mut utf8_of: FxHashMap<&str, u16> = HashMap::default();
        let mut class_of: FxHashMap<&str, u16> = HashMap::default();
        let mut nat_of: FxHashMap<(&str, &str), u16> = HashMap::default();
        for (i, e) in self.entries.iter().enumerate() {
            let slot = self.slots[i];
            match e {
                Entry::Utf8(s) => {
                    utf8_of.insert(s.as_str(), slot);
                }
                Entry::Class(n) => {
                    class_of.insert(n.as_str(), slot);
                }
                Entry::NameAndType(n, d) => {
                    nat_of.insert((n.as_str(), d.as_str()), slot);
                }
                _ => {}
            }
        }

        buf.u16(self.count());
        for e in &self.entries {
            match e {
                Entry::Utf8(s) => {
                    buf.u8(1);
                    // JVM modified UTF-8. ASCII is identical; good enough for now.
                    let bytes = s.as_bytes();
                    buf.u16(bytes.len() as u16);
                    buf.bytes(bytes);
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
                    buf.u16(utf8_of[n.as_str()]);
                }
                Entry::NameAndType(n, d) => {
                    buf.u8(12);
                    buf.u16(utf8_of[n.as_str()]);
                    buf.u16(utf8_of[d.as_str()]);
                }
                Entry::Fieldref(o, n, d) => {
                    buf.u8(9);
                    buf.u16(class_of[o.as_str()]);
                    buf.u16(nat_of[&(n.as_str(), d.as_str())]);
                }
                Entry::Methodref(o, n, d) => {
                    buf.u8(10);
                    buf.u16(class_of[o.as_str()]);
                    buf.u16(nat_of[&(n.as_str(), d.as_str())]);
                }
                Entry::StringConst(s) => {
                    buf.u8(8);
                    buf.u16(utf8_of[s.as_str()]);
                }
            }
        }
    }
}

/// A `verification_type_info` in a StackMapTable frame — the verifier's view of
/// one local or stack slot. Only the variants the numeric+branch subset produces;
/// the four small integral types (`boolean`/`byte`/`char`/`short`/`int`) all map
/// to `Integer`. `Object` carries the referenced class's internal name, resolved
/// to its constant-pool `Class` index when the frame is serialized. A `Long`/
/// `Double` is a **single** entry even though it occupies two JVM slots.
#[derive(Clone, PartialEq, Eq)]
pub enum VerificationType {
    Integer,      // tag 1
    Float,        // tag 2
    Double,       // tag 3
    Long,         // tag 4
    Object(String), // tag 7 + u2 Class index
}

impl VerificationType {
    fn write(&self, buf: &mut ByteBuf, cp: &ConstantPool) {
        match self {
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

/// One method: fully lowered bytecode plus the metadata needed to write it.
pub struct Method {
    pub access_flags: u16,
    pub name: String,
    pub descriptor: String,
    pub max_stack: u16,
    pub max_locals: u16,
    pub code: Vec<u8>,
    /// (start_pc, line_number) pairs for the LineNumberTable attribute.
    pub line_numbers: Vec<(u16, u16)>,
    /// Verifier state at method entry (the parameter locals). Only the seed for
    /// the first frame's delta/append computation; never serialized on its own.
    pub entry_locals: Vec<VerificationType>,
    /// StackMapTable frames in increasing-offset order. Empty ⇒ the method is
    /// straight-line and no StackMapTable attribute (nor its Utf8) is emitted.
    pub stack_frames: Vec<StackFrame>,
}

pub struct ClassFile {
    pub access_flags: u16,
    pub this_class: String,
    pub super_class: String,
    pub source_file: String,
    pub methods: Vec<Method>,
}

impl ClassFile {
    /// Serialize the whole file. `cp` must already contain the phase-1
    /// (code-generation) constants, interned in bytecode-reference order by the
    /// caller. Here we add the phase-2 (writing-order) structural constants and
    /// emit the bytes.
    pub fn to_bytes(&self, mut cp: ConstantPool) -> Vec<u8> {
        // Phase 2: writing order.
        let this_idx = cp.class(&self.this_class);
        let super_idx = cp.class(&self.super_class);

        // Per-method structural Utf8s, in declaration order. `StackMapTable` is
        // interned right after the method's name/descriptor and the standing
        // `Code`/`LineNumberTable` names, and only when the method has frames —
        // exactly javac's pool order and its "omit the Utf8 for straight-line
        // methods" rule.
        struct MethodIdx {
            name: u16,
            descriptor: u16,
            code_attr: u16,
            line_attr: u16,
            stack_map_attr: Option<u16>,
        }
        let mut method_idx = Vec::new();
        for m in &self.methods {
            let name = cp.utf8(&m.name);
            let descriptor = cp.utf8(&m.descriptor);
            let code_attr = cp.utf8("Code");
            let line_attr = cp.utf8("LineNumberTable");
            let stack_map_attr = (!m.stack_frames.is_empty()).then(|| cp.utf8("StackMapTable"));
            // Any `Class` a frame names (only `args`'s array type here, and only
            // when a full_frame lists it) is interned right after the attribute
            // name — javac's pool order.
            if stack_map_attr.is_some() {
                for name in frame_object_classes(&m.stack_frames, &m.entry_locals) {
                    cp.class(&name);
                }
            }
            method_idx.push(MethodIdx { name, descriptor, code_attr, line_attr, stack_map_attr });
        }

        // Class attribute names.
        let sourcefile_attr = cp.utf8("SourceFile");
        let sourcefile_val = cp.utf8(&self.source_file);

        // ---- serialize ----
        let mut buf = ByteBuf::new();
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
        for (m, mi) in self.methods.iter().zip(&method_idx) {
            buf.u16(m.access_flags);
            buf.u16(mi.name);
            buf.u16(mi.descriptor);
            buf.u16(1); // attributes_count: just Code

            // The StackMapTable body (number_of_entries + frames), if any. Built
            // first so it can be both measured and written.
            let smt_body = mi
                .stack_map_attr
                .map(|_| stack_map_body(&m.stack_frames, &m.entry_locals, &cp));

            // Code attribute.
            // body: max_stack(2) max_locals(2) code_len(4) code exc_len(2)
            //       attrs_count(2) + LineNumberTable attribute [+ StackMapTable]
            let line_attr_len = 2 + 4 * m.line_numbers.len();
            let smt_attr_len = smt_body.as_ref().map_or(0, |b| 6 + b.len());
            let code_attrs = 1 + smt_body.is_some() as u16; // LineNumberTable [+ StackMapTable]
            let code_attr_len = 2 + 2 + 4 + m.code.len() + 2 + 2 + (6 + line_attr_len) + smt_attr_len;
            buf.u16(mi.code_attr);
            buf.u32(code_attr_len as u32);
            buf.u16(m.max_stack);
            buf.u16(m.max_locals);
            buf.u32(m.code.len() as u32);
            buf.bytes(&m.code);
            buf.u16(0); // exception_table_length
            buf.u16(code_attrs);

            // LineNumberTable first, then StackMapTable — javac's sub-attribute order.
            buf.u16(mi.line_attr);
            buf.u32(line_attr_len as u32);
            buf.u16(m.line_numbers.len() as u16);
            for &(pc, line) in &m.line_numbers {
                buf.u16(pc);
                buf.u16(line);
            }
            if let (Some(attr), Some(body)) = (mi.stack_map_attr, &smt_body) {
                buf.u16(attr);
                buf.u32(body.len() as u32);
                buf.bytes(body);
            }
        }

        buf.u16(1); // class attributes_count: SourceFile
        buf.u16(sourcefile_attr);
        buf.u32(2); // SourceFile length
        buf.u16(sourcefile_val);

        buf.into_vec()
    }
}

/// Serialize a method's StackMapTable attribute *body* (`number_of_entries`
/// followed by the frames) into a fresh buffer. Returns the bytes so the caller
/// can both measure (`attribute_length`) and emit them.
///
/// For each frame the `offset_delta` uses javac's rule — the first frame's delta
/// is its absolute offset; every later frame's is `offset − prevOffset − 1` (the
/// −1 inter-frame bias) — and the smallest frame form that expresses the change
/// from the previous frame's state is chosen (`same` / `same_locals_1_stack_item`
/// / `append` / `chop`, falling back to `full_frame`), exactly as javac does.
fn stack_map_body(frames: &[StackFrame], entry_locals: &[VerificationType], cp: &ConstantPool) -> Vec<u8> {
    let mut buf = ByteBuf::new();
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
                vt.write(&mut buf, cp);
            }
            FrameShape::SameLocals1(vt) => {
                buf.u8(247); // same_locals_1_stack_item_frame_extended
                buf.u16(delta);
                vt.write(&mut buf, cp);
            }
            FrameShape::Append(new) => {
                buf.u8(251 + new.len() as u8); // append_frame (k = 1..=3)
                buf.u16(delta);
                for vt in new {
                    vt.write(&mut buf, cp);
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
                    vt.write(&mut buf, cp);
                }
                buf.u16(f.stack.len() as u16);
                for vt in &f.stack {
                    vt.write(&mut buf, cp);
                }
            }
        }

        prev_offset = Some(f.offset);
        prev_locals = &f.locals;
    }
    buf.into_vec()
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
    pub fn new() -> Self {
        ByteBuf(Vec::new())
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
    pub fn bytes(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}
