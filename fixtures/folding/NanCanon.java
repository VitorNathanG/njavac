// Regression: NaN canonicalization in the constant pool.
// A NaN produced by folding and then pushed through one more operation keeps a
// sign-flipped bit pattern (`0xFFC00000` / `0xFFF80000...`), but javac writes float/
// double constants through Float.floatToIntBits / Double.doubleToLongBits, which
// collapse every NaN to the canonical `0x7FC00000` / `0x7FF8000000000000`. Each line
// folds to a NaN whose raw bits differ from the canonical form; `-0.0` must stay a
// distinct (non-NaN) entry. Fuzzer-found (signatures cp[N].float_bits / double_hi).
public class NanCanon {
    public static void main(String[] args) {
        float f0 = -(0.0f / 0.0f);   // -NaN via negation
        double d0 = -(10.0 % 0.0);   // -NaN via remainder then negation
        float f1 = -0.0f;            // NOT a NaN: canonicalization must leave it alone
        double d1 = -0.0;            // same, double
    }
}
