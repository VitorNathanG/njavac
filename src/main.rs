//! njavac — a toy Java 25 compiler.
//!
//! Milestone 1: emit `HelloWorld.class` byte-identical to `javac 25`.
//! There is no parser yet; the HelloWorld program is hand-lowered so we can
//! nail the class-file writer and constant-pool ordering first.

mod classfile;

use classfile::{ByteBuf, ClassFile, ConstantPool, Method};
use std::io::Write;

// JVM opcodes used here.
const ALOAD_0: u8 = 0x2a;
const INVOKESPECIAL: u8 = 0xb7;
const RETURN: u8 = 0xb1;
const GETSTATIC: u8 = 0xb2;
const LDC: u8 = 0x12;
const LDC_W: u8 = 0x13;
const INVOKEVIRTUAL: u8 = 0xb6;

const ACC_PUBLIC: u16 = 0x0001;
const ACC_STATIC: u16 = 0x0008;
const ACC_SUPER: u16 = 0x0020;

fn build_hello_world() -> Vec<u8> {
    let mut cp = ConstantPool::new();

    // ---- Phase 1: code generation, in declaration order (implicit <init> first).
    // Operands are interned here, populating the pool in bytecode-reference order.

    // Default constructor: aload_0; invokespecial Object.<init>; return
    let init_code = {
        let obj_init = cp.methodref("java/lang/Object", "<init>", "()V");
        let mut c = ByteBuf::new();
        c.u8(ALOAD_0);
        c.u8(INVOKESPECIAL);
        c.u16(obj_init);
        c.u8(RETURN);
        c.into_vec()
    };

    // main: getstatic System.out; ldc "Hello, World!"; invokevirtual println; return
    let main_code = {
        let sysout = cp.fieldref("java/lang/System", "out", "Ljava/io/PrintStream;");
        let hello = cp.string("Hello, World!");
        let println = cp.methodref("java/io/PrintStream", "println", "(Ljava/lang/String;)V");
        let mut c = ByteBuf::new();
        c.u8(GETSTATIC);
        c.u16(sysout);
        // javac uses the 1-byte `ldc` for pool indices that fit in a u8.
        if hello <= 0xff {
            c.u8(LDC);
            c.u8(hello as u8);
        } else {
            c.u8(LDC_W);
            c.u16(hello);
        }
        c.u8(INVOKEVIRTUAL);
        c.u16(println);
        c.u8(RETURN);
        c.into_vec()
    };

    let methods = vec![
        Method {
            access_flags: ACC_PUBLIC,
            name: "<init>".to_string(),
            descriptor: "()V".to_string(),
            max_stack: 1,
            max_locals: 1,
            code: init_code,
            line_numbers: vec![(0, 1)],
        },
        Method {
            access_flags: ACC_PUBLIC | ACC_STATIC,
            name: "main".to_string(),
            descriptor: "([Ljava/lang/String;)V".to_string(),
            max_stack: 2,
            max_locals: 1,
            code: main_code,
            line_numbers: vec![(0, 3), (8, 4)],
        },
    ];

    let class = ClassFile {
        access_flags: ACC_PUBLIC | ACC_SUPER,
        this_class: "HelloWorld".to_string(),
        super_class: "java/lang/Object".to_string(),
        source_file: "HelloWorld.java".to_string(),
        methods,
    };

    // ---- Phase 2 + serialize (uses the pool we just populated).
    class.to_bytes(cp)
}

fn main() -> std::io::Result<()> {
    let bytes = build_hello_world();
    let out = std::env::args().nth(1).unwrap_or_else(|| "HelloWorld.class".to_string());
    let mut f = std::fs::File::create(&out)?;
    f.write_all(&bytes)?;
    eprintln!("wrote {} ({} bytes)", out, bytes.len());
    Ok(())
}
