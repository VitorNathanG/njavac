// Persistent in-memory javac worker for the differential fuzzer.
//
// WHY THIS EXISTS. The fuzzer's wall time was dominated by re-spawning `javac`:
// each spawn pays ~0.3s JVM launch AND — the bigger cost — javac re-JIT-warming
// its whole front-end from cold, a cost that only exists once per JVM lifetime.
// Batching amortized the LAUNCH but not the re-warm. This worker keeps ONE JVM
// hot for the entire run: sources arrive over stdin, `.class` bytes leave over
// stdout, and javac NEVER TOUCHES DISK (no source files written, no class files
// written/read/scanned) — it compiles from an in-memory JavaFileObject straight
// into an in-memory byte buffer.
//
// BYTE-IDENTITY IS THE WHOLE POINT, so this must produce the EXACT bytes the
// `javac` CLI (`javac -d <dir> Name.java`) produces, or it is worthless as the
// fuzzer's oracle. The fuzzer's `--verify-worker` mode is the empirical proof:
// it compares worker acceptance and bytes with real pinned CLI invocations and
// must be rerun after every JDK or worker change. Two inputs are kept aligned:
//   1. Compiler options. We pass no options, matching the CLI invocation used by
//      the verification gate.
//   2. Source-file name. The `.class` couples to the source name via the
//      SourceFile attribute (and LineNumberTable). The in-memory source is named
//      "<Class>.java" (URI string:///<Class>.java, getName() "<Class>.java"), so
//      javac's simple-name derivation yields "<Class>.java" — identical to a real
//      file the CLI would open.
//
// ISOLATION. A fresh compilation task is created per request. The JVM and
// StandardJavaFileManager are reused for speed, so empirical CLI equivalence is a
// required gate rather than an assumption about implementation internals.
//
// BATCHING. Compiling each program in a separate task is much slower, so a request
// carries many units and compiles them in one task. The reply is the set of classes
// produced; the fuzzer maps each class back by name and treats an expected class
// that is absent as that unit's javac rejection.
//
// PROTOCOL (big-endian, DataInput/DataOutputStream on both ends; lock-step
// request→response). Request:  int nUnits, then per unit { int nameLen, name utf8,
// int srcLen, src utf8 }. Response: int nClasses, then per class { int nameLen,
// binary-class-name utf8, int len, class bytes }. The fuzzer rejects every returned
// class name not expected for the batch. Missing expected names represent rejected
// units and are not a protocol error.
import java.io.*;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.*;
import javax.tools.*;

public final class FuzzJavac {
    public static void main(String[] args) throws IOException {
        JavaCompiler comp = ToolProvider.getSystemJavaCompiler();
        if (comp == null) { System.err.println("FuzzJavac: no system java compiler"); System.exit(2); }
        StandardJavaFileManager std = comp.getStandardFileManager(null, null, StandardCharsets.UTF_8);

        // Bind the protocol to the raw stdin/stdout FDs, THEN muzzle System.out/err
        // so a stray println from anywhere can never corrupt the binary stream.
        DataInputStream in = new DataInputStream(new BufferedInputStream(new FileInputStream(FileDescriptor.in)));
        DataOutputStream out = new DataOutputStream(new BufferedOutputStream(new FileOutputStream(FileDescriptor.out)));
        System.setOut(new PrintStream(OutputStream.nullOutputStream()));
        System.setErr(new PrintStream(OutputStream.nullOutputStream()));

        while (true) {
            int nUnits;
            try {
                nUnits = in.readInt();
            } catch (EOFException eof) {
                break; // parent closed stdin -> clean shutdown
            }
            List<JavaFileObject> units = new ArrayList<>(nUnits);
            for (int i = 0; i < nUnits; i++) {
                String name = readStr(in);
                String src = readStr(in);
                units.add(new MemSource(name, src));
            }

            MemFileManager fm = new MemFileManager(std);
            try {
                // null Writer (compiler chatter), no-op diagnostic sink, NO options
                // (match the CLI defaults), no extra classes — the whole batch in
                // ONE task. Errors in some units don't stop classfile output for the
                // rest (call() returns false but the good classes are still written).
                comp.getTask(null, fm, d -> {}, null, null, units).call();
            } catch (RuntimeException e) {
                // A compiler blowup leaves whatever was already written; report that
                // partial set rather than hang. (Never observed in-subset.)
            }

            Map<String, byte[]> classes = fm.outputs();
            out.writeInt(classes.size());
            for (Map.Entry<String, byte[]> e : classes.entrySet()) {
                writeStr(out, e.getKey());
                out.writeInt(e.getValue().length);
                out.write(e.getValue());
            }
            out.flush();
        }
    }

    private static String readStr(DataInputStream in) throws IOException {
        int len = in.readInt();               // throws EOFException at end of stream
        byte[] b = new byte[len];
        in.readFully(b);
        return new String(b, StandardCharsets.UTF_8);
    }

    private static void writeStr(DataOutputStream out, String s) throws IOException {
        byte[] b = s.getBytes(StandardCharsets.UTF_8);
        out.writeInt(b.length);
        out.write(b);
    }

    /// An in-memory source unit named "<Class>.java" so javac derives the exact
    /// SourceFile attribute a real file would (see the byte-identity note above).
    private static final class MemSource extends SimpleJavaFileObject {
        private final String code;
        private final String fileName;
        MemSource(String className, String code) {
            super(URI.create("string:///" + className + ".java"), Kind.SOURCE);
            this.fileName = className + ".java";
            this.code = code;
        }
        @Override public CharSequence getCharContent(boolean ignoreEncodingErrors) { return code; }
        @Override public String getName() { return fileName; }
    }

    /// Captures every emitted class into a byte buffer instead of a `-d` directory.
    /// Overriding getJavaFileForOutput fully (not delegating) means no output
    /// location need be configured and nothing is written to disk. LinkedHashMap
    /// keeps emission order stable for reproducible responses.
    private static final class MemFileManager
            extends ForwardingJavaFileManager<StandardJavaFileManager> {
        private final Map<String, ByteArrayOutputStream> out = new LinkedHashMap<>();
        MemFileManager(StandardJavaFileManager delegate) { super(delegate); }

        @Override
        public JavaFileObject getJavaFileForOutput(Location location, String className,
                                                   JavaFileObject.Kind kind, FileObject sibling) {
            ByteArrayOutputStream bos = new ByteArrayOutputStream();
            out.put(className, bos);
            URI uri = URI.create("mem:///" + className.replace('.', '/') + kind.extension);
            return new SimpleJavaFileObject(uri, kind) {
                @Override public OutputStream openOutputStream() { return bos; }
            };
        }

        Map<String, byte[]> outputs() {
            Map<String, byte[]> m = new LinkedHashMap<>();
            for (Map.Entry<String, ByteArrayOutputStream> e : out.entrySet()) {
                m.put(e.getKey(), e.getValue().toByteArray());
            }
            return m;
        }
    }
}
