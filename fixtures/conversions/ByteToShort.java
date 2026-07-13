// Regression: byte->short is a *widening* conversion, yet javac still emits `i2s`.
// javac's Items.coerce narrows to the target sub-int type whenever the source and
// target typecodes differ (byte/char/short collapse to int under Code.truncate, so
// the ONLY no-op is same-typecode-to-same). So byte->short — numerically a no-op,
// since a byte already fits a short — nonetheless emits `i2s`, in an explicit cast
// AND in an implicit assignment. njavac used to skip it (it treated byte as already
// fitting short), emitting a Code attribute one `i2s` byte short. Fuzzer-found
// (Fuzz0000004: `short v4 = (short)((byte) aDouble)`).
public class ByteToShort {
    public static void main(String[] args) {
        byte b = 1;
        double d = 5.0;
        short s1 = b;                    // implicit byte->short widening: iload; i2s; istore
        short s2 = (short) b;            // explicit byte->short cast:      iload; i2s; istore
        short s3 = (short) ((byte) d);   // double->byte->short:            d2i; i2b; i2s; istore
        byte b2 = b;                     // byte->byte: no conversion op (contrast: same typecode)
        System.out.println(s1);
        System.out.println(s2);
        System.out.println(s3);
        System.out.println(b2);
    }
}
