// Persistent execution observer for pairs of class files.
//
// Protocol (big-endian int32 lengths): each request is a framed UTF-8 binary
// class name, framed reference class bytes, and framed candidate class bytes.
// Each response contains two observations, reference first. An observation is
// int status followed by framed stdout, stderr, and UTF-8 detail bytes.
import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.EOFException;
import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

public final class FuzzObserve {
    private static final int RETURNED = 0;
    private static final int THREW = 1;
    private static final int LOAD_FAILED = 2;
    private static final int TIMED_OUT = 3;
    private static final int NOT_RUN = 4;
    private static final long TIMEOUT_SECONDS = 2;
    private static final int READY = 0x4e4a4f42;
    private static final int MAX_OUTPUT_BYTES = 1 << 20;

    private static final PrintStream NULL_STREAM =
            new PrintStream(OutputStream.nullOutputStream(), true, StandardCharsets.UTF_8);

    public static void main(String[] args) throws IOException {
        // Keep the protocol streams attached to the raw descriptors. Observed
        // classes may write to System.out and System.err without corrupting it.
        DataInputStream in = new DataInputStream(
                new BufferedInputStream(new FileInputStream(FileDescriptor.in)));
        DataOutputStream out = new DataOutputStream(
                new BufferedOutputStream(new FileOutputStream(FileDescriptor.out)));
        System.setOut(NULL_STREAM);
        System.setErr(NULL_STREAM);
        System.setIn(InputStream.nullInputStream());
        out.writeInt(READY);
        out.flush();

        while (true) {
            String name;
            try {
                name = readString(in);
            } catch (EOFException eof) {
                return;
            }
            byte[] referenceBytes = readFrame(in);
            byte[] candidateBytes = readFrame(in);

            Result reference = runOne(name, referenceBytes);
            Result candidate = reference.status == TIMED_OUT
                    ? Result.notRun()
                    : runOne(name, candidateBytes);
            reference.writeTo(out);
            candidate.writeTo(out);
            out.flush();

            if (reference.status == TIMED_OUT || candidate.status == TIMED_OUT) {
                Runtime.getRuntime().halt(124);
            }
        }
    }

    private static Result runOne(String name, byte[] classBytes) {
        SyncBuffer stdout = new SyncBuffer();
        SyncBuffer stderr = new SyncBuffer();
        PrintStream capturedOut = new PrintStream(stdout, true, StandardCharsets.UTF_8);
        PrintStream capturedErr = new PrintStream(stderr, true, StandardCharsets.UTF_8);
        FutureTask<Outcome> task = new FutureTask<>(() -> invoke(name, classBytes));
        Thread thread = new Thread(task, "fuzz-observe-class");
        thread.setDaemon(true);

        System.setOut(capturedOut);
        System.setErr(capturedErr);
        thread.start();

        Outcome outcome;
        try {
            outcome = task.get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
        } catch (TimeoutException e) {
            task.cancel(true);
            outcome = new Outcome(TIMED_OUT, "");
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            outcome = new Outcome(LOAD_FAILED, throwableDetail(e));
        } catch (ExecutionException e) {
            outcome = new Outcome(LOAD_FAILED, throwableDetail(e.getCause()));
        } finally {
            System.setOut(NULL_STREAM);
            System.setErr(NULL_STREAM);
        }

        return new Result(outcome.status, stdout.snapshot(), stderr.snapshot(), outcome.detail);
    }

    private static Outcome invoke(String name, byte[] classBytes) {
        try {
            ByteClassLoader loader = new ByteClassLoader();
            Thread.currentThread().setContextClassLoader(loader);
            Class<?> type = loader.define(name, classBytes);
            Method main = type.getMethod("main", String[].class);
            try {
                main.invoke(null, (Object) new String[0]);
                return new Outcome(RETURNED, "");
            } catch (InvocationTargetException e) {
                Throwable cause = e.getCause() == null ? e : e.getCause();
                return new Outcome(THREW, throwableDetail(cause));
            }
        } catch (Throwable t) {
            return new Outcome(LOAD_FAILED, throwableDetail(t));
        }
    }

    private static String throwableDetail(Throwable t) {
        if (t == null) {
            return "java.lang.Throwable";
        }
        String message = t.getMessage();
        return message == null || message.isEmpty()
                ? t.getClass().getName()
                : t.getClass().getName() + ": " + message;
    }

    private static String readString(DataInputStream in) throws IOException {
        return new String(readFrame(in), StandardCharsets.UTF_8);
    }

    private static byte[] readFrame(DataInputStream in) throws IOException {
        int length = in.readInt();
        if (length < 0) {
            throw new IOException("negative frame length");
        }
        byte[] bytes = new byte[length];
        in.readFully(bytes);
        return bytes;
    }

    private static void writeFrame(DataOutputStream out, byte[] bytes) throws IOException {
        out.writeInt(bytes.length);
        out.write(bytes);
    }

    private static final class ByteClassLoader extends ClassLoader {
        ByteClassLoader() {
            super(ClassLoader.getPlatformClassLoader());
        }

        Class<?> define(String name, byte[] bytes) {
            return defineClass(name, bytes, 0, bytes.length);
        }
    }

    private record Outcome(int status, String detail) {}

    private record Result(int status, byte[] stdout, byte[] stderr, String detail) {
        static Result notRun() {
            return new Result(NOT_RUN, new byte[0], new byte[0], "");
        }

        void writeTo(DataOutputStream out) throws IOException {
            out.writeInt(status);
            writeFrame(out, stdout);
            writeFrame(out, stderr);
            writeFrame(out, detail.getBytes(StandardCharsets.UTF_8));
        }
    }

    private static final class SyncBuffer extends ByteArrayOutputStream {
        @Override
        public synchronized void write(int value) {
            if (count < MAX_OUTPUT_BYTES) {
                super.write(value);
            }
        }

        @Override
        public synchronized void write(byte[] bytes, int offset, int length) {
            int remaining = MAX_OUTPUT_BYTES - count;
            if (remaining > 0) {
                super.write(bytes, offset, Math.min(length, remaining));
            }
        }

        synchronized byte[] snapshot() {
            return toByteArray();
        }
    }
}
