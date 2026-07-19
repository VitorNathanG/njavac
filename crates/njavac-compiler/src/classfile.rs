//! Class file model + serializer.
//!
//! The tricky part of matching `javac` byte-for-byte is the constant pool:
//! entries must appear in the *exact* order javac emits them. Empirically
//! Empirical fixture and probe output shows that javac interns each composite entry
//! breadth-first: a Methodref takes its own slot, then its Class and
//! NameAndType take slots, then *their* Utf8 children, and so on. We
//! reproduce that with a FIFO queue per top-level intern call.

mod buffer;
mod model;
mod modified_utf8;
mod pool;
mod writer;

pub use model::{Attribute, ClassFile, CodeAttribute, Method, StackFrame, VerificationType};
pub use pool::ConstantPool;
