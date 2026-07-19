/// A `verification_type_info` in a StackMapTable frame — the verifier's view of
/// one local or stack slot. The four small integral types
/// (`boolean`/`byte`/`char`/`short`/`int`) all map to `Integer`; `Top` preserves an
/// interior physical slot for an uninitialized local. `Object` carries the
/// referenced class's internal name, resolved to its constant-pool `Class` index
/// when the frame is serialized. A `Long`/`Double` is a **single** entry even
/// though it occupies two JVM slots.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VerificationType {
    Top,            // tag 0
    Integer,        // tag 1
    Float,          // tag 2
    Double,         // tag 3
    Long,           // tag 4
    Object(String), // tag 7 + u2 Class index
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
}
