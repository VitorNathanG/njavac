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

/// A logical constant-pool entry, keyed by its owned contents so we can dedup
/// (intern) identical entries. Child references are stored as keys and resolved
/// to indices at serialization time via the intern map.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Entry {
    Utf8(String),
    /// A CONSTANT_Integer: a 4-byte `int` value. A leaf (no children).
    Integer(i32),
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
            Entry::Utf8(_) => vec![],
            Entry::Integer(_) => vec![],
            Entry::Class(n) => vec![Entry::Utf8(n.clone())],
            Entry::NameAndType(n, d) => vec![Entry::Utf8(n.clone()), Entry::Utf8(d.clone())],
            Entry::Fieldref(o, n, d) | Entry::Methodref(o, n, d) => vec![
                Entry::Class(o.clone()),
                Entry::NameAndType(n.clone(), d.clone()),
            ],
            Entry::StringConst(s) => vec![Entry::Utf8(s.clone())],
        }
    }
}

pub struct ConstantPool {
    entries: Vec<Entry>,
    index: HashMap<Entry, u16>,
}

impl ConstantPool {
    pub fn new() -> Self {
        ConstantPool { entries: Vec::new(), index: HashMap::new() }
    }

    fn idx_of(&self, e: &Entry) -> u16 {
        *self.index.get(e).expect("entry must be interned before lookup")
    }

    /// Number stored in the class file: entry count + 1 (slot 0 is reserved).
    pub fn count(&self) -> u16 {
        self.entries.len() as u16 + 1
    }

    /// Append a single entry (no child handling), assigning it the next 1-based slot.
    fn alloc(&mut self, e: Entry) -> u16 {
        if let Some(&i) = self.index.get(&e) {
            return i;
        }
        let idx = self.entries.len() as u16 + 1;
        self.entries.push(e.clone());
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
                Entry::Class(n) => {
                    buf.u8(7);
                    buf.u16(self.idx_of(&Entry::Utf8(n.clone())));
                }
                Entry::NameAndType(n, d) => {
                    buf.u8(12);
                    buf.u16(self.idx_of(&Entry::Utf8(n.clone())));
                    buf.u16(self.idx_of(&Entry::Utf8(d.clone())));
                }
                Entry::Fieldref(o, n, d) => {
                    buf.u8(9);
                    buf.u16(self.idx_of(&Entry::Class(o.clone())));
                    buf.u16(self.idx_of(&Entry::NameAndType(n.clone(), d.clone())));
                }
                Entry::Methodref(o, n, d) => {
                    buf.u8(10);
                    buf.u16(self.idx_of(&Entry::Class(o.clone())));
                    buf.u16(self.idx_of(&Entry::NameAndType(n.clone(), d.clone())));
                }
                Entry::StringConst(s) => {
                    buf.u8(8);
                    buf.u16(self.idx_of(&Entry::Utf8(s.clone())));
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
