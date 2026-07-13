// Regression: compound assignment of a NEGATIVE constant on a narrowing target.
// javac normalizes `x op= c` (c constant, int-family target) to a non-negative
// magnitude and picks the operator by the effective delta's sign, so `char v -= -100`
// emits `bipush 100; iadd; i2c` — NOT the raw `bipush -100; isub; i2c`. njavac already
// did this on the `int` iinc-overflow path, but the general narrowing path (char/
// short/byte) emitted the raw negative constant + `isub`. The normalization is
// int-family only: a long/float/double target keeps the raw `lsub`/`dsub`. Fuzzer-
// found (signature cp[N].int). The last two lines guard against over-normalizing.
public class CompoundNegConst {
    public static void main(String[] args) {
        char  c = 'a';  c -= -100;      // narrowing + negative delta -> bipush 100; iadd; i2c
        short s = 1;    s -= -30000;    // sipush 30000; iadd; i2s
        byte  b = 5;    b -= -100;      // bipush 100; iadd; i2b
        char  d = 'a';  d += -50;       // += negative -> isub side: bipush 50; isub; i2c
        char  e = 'a';  e -= -40000;    // magnitude needs ldc: ldc 40000; iadd; i2c
        long  lg = 5;   lg -= -5L;      // int-family ONLY: stays raw ldc2_w -5l; lsub
        double db = 5;  db -= -2.0;     // stays raw ldc2_w -2.0d; dsub
    }
}
