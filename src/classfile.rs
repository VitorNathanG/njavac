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
    /// A CONSTANT_Float, stored as its raw 32-bit pattern so that `-0.0f`/`NaN`
    /// dedup by bits (a distinct entry from `+0.0f`), matching javac. A leaf.
    Float(u32),
    /// A CONSTANT_Double, stored as its raw 64-bit pattern (same bit-dedup rule).
    /// A leaf; **occupies two pool indices**.
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
        self.intern(Entry::Float(v.to_bits()))
    }
    pub fn double(&mut self, v: f64) -> u16 {
        self.intern(Entry::Double(v.to_bits()))
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

        // Per-method structural Utf8s, in declaration order.
        struct MethodIdx {
            name: u16,
            descriptor: u16,
            code_attr: u16,
            line_attr: u16,
        }
        let mut method_idx = Vec::new();
        for m in &self.methods {
            let name = cp.utf8(&m.name);
            let descriptor = cp.utf8(&m.descriptor);
            let code_attr = cp.utf8("Code");
            let line_attr = cp.utf8("LineNumberTable");
            method_idx.push(MethodIdx { name, descriptor, code_attr, line_attr });
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

            // Code attribute.
            // body: max_stack(2) max_locals(2) code_len(4) code exc_len(2)
            //       attrs_count(2) + LineNumberTable attribute
            let line_attr_len = 2 + 4 * m.line_numbers.len();
            let code_attr_len = 2 + 2 + 4 + m.code.len() + 2 + 2 + (6 + line_attr_len);
            buf.u16(mi.code_attr);
            buf.u32(code_attr_len as u32);
            buf.u16(m.max_stack);
            buf.u16(m.max_locals);
            buf.u32(m.code.len() as u32);
            buf.bytes(&m.code);
            buf.u16(0); // exception_table_length
            buf.u16(1); // attributes_count: LineNumberTable
            buf.u16(mi.line_attr);
            buf.u32(line_attr_len as u32);
            buf.u16(m.line_numbers.len() as u16);
            for &(pc, line) in &m.line_numbers {
                buf.u16(pc);
                buf.u16(line);
            }
        }

        buf.u16(1); // class attributes_count: SourceFile
        buf.u16(sourcefile_attr);
        buf.u32(2); // SourceFile length
        buf.u16(sourcefile_val);

        buf.into_vec()
    }
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
