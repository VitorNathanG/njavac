// Regression: the i32::MIN compound-assign delta corner.
// `x -= -2147483648` and `x += -2147483648` both yield an increment whose magnitude
// (i32::MIN) is unrepresentable. javac emits `ldc -2147483648; isub` in BOTH cases
// (x + MIN == x - MIN mod 2^32), never `iadd i32::MIN`. Fuzzer-found (Fuzz0001145);
// the magnitude-normalization path used to pick `iadd` for this one delta. Covers an
// int target and a narrowing (byte) target, `+=` and `-=`.
public class MinDeltaCompound {
    public static void main(String[] args) {
        int  a = 0;  a -= -2147483648;   // ldc -2147483648; isub
        int  b = 0;  b += -2147483648;   // ldc -2147483648; isub (isub even for +=)
        byte c = 0;  c -= -2147483648;   // ldc -2147483648; isub; i2b
        byte d = 0;  d += -2147483648;   // ldc -2147483648; isub; i2b
    }
}
