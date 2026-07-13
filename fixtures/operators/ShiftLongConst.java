// Regression: `long >>> long` folding + constant shift-distance narrowing.
// Two coupled javac behaviors the fuzzer surfaced (complements ShiftAmt, which
// covers non-constant amounts):
//   (1) javac constant-folds EVERY shift except `long >>> long` (unsigned shift,
//       both operands statically `long`) — a genuine ConstFold quirk. njavac used
//       to over-fold it to a constant.
//   (2) a CONSTANT shift distance is narrowed to an int constant (`bipush 40`),
//       never `ldc2_w <long>; l2i`. njavac used to push the long distance + l2i
//       (an extra Long pool entry + wrong max_stack).
// The "still folds" lines guard against over-refusing (only long>>>long is special).
// Fuzzer-found (the whole tail: constant_pool_count / attr length / cp[N].long_*).
public class ShiftLongConst {
    public static void main(String[] args) {
        int x = 7;
        int y = x << 40L;                      // (2) non-const left: bipush 40; ishl (no l2i, no Long)
        int z = 3;  z <<= 40L;                 // (2) same on the compound-assign path
        long a = 127L >>> 62L;                 // (1) long>>>long: NOT folded -> ldc2_w 127l; bipush 62; lushr
        long b = 9223372036854775807L >>> 1L;  // (1) still not folded
        long c = 5L << -2L;                    // folds (long<<long): ldc2_w 4611686018427387904l
        long d = 127L >> 3L;                   // folds (long>>long, signed): ldc2_w 15l
        long e = 127L >>> 4;                   // folds (long>>>int: distance is int): ldc2_w 7l
        int  f = 10 >>> 40L;                   // folds (int>>>long: left is int): iconst_0
    }
}
