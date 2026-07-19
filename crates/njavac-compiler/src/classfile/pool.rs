use std::collections::VecDeque;
use std::rc::Rc;

use crate::fxhash::FxHashMap;

use super::buffer::ByteBuf;
use super::modified_utf8;

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

    pub(super) fn serialize(&self, buf: &mut ByteBuf) {
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
