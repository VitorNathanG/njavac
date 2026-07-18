# njavac Maintainer Guide

This book is the authoritative maintainer documentation for njavac. The full
guide is being migrated into this structure; until that migration is complete,
the root documentation remains in force.

```mermaid
flowchart LR
    Source[Java source] --> Lexer
    Lexer --> Parser
    Parser --> Sema[Semantic analysis]
    Sema --> Lowering
    Lowering --> Assembler
    Assembler --> Writer[Class-file writer]
    Writer --> Bytes[.class bytes]
```
