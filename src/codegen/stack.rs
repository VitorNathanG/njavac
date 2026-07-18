use crate::ast::PrimitiveType;

/// The four JVM operand-stack computational types produced by primitive values.
/// References stay in `Type` and never enter numeric opcode selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum StackTy {
    Int,
    Long,
    Float,
    Double,
}

impl PrimitiveType {
    /// The JVM computational type this value occupies. The sub-int types
    /// (`boolean`/`char`/`byte`/`short`) are all `Int` on the operand stack.
    pub(super) fn stack(self) -> StackTy {
        match self {
            PrimitiveType::Long => StackTy::Long,
            PrimitiveType::Float => StackTy::Float,
            PrimitiveType::Double => StackTy::Double,
            _ => StackTy::Int,
        }
    }
}
