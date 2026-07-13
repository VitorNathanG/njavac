public class CastProbe {
    public static void main(String[] args) {
        byte b = 1;
        double d = 5.0;
        char c = 'x';
        short s1 = b;                    // implicit byte->short widening
        short s2 = (short) b;            // explicit byte->short cast
        short s4 = (short)((byte) d);    // the fuzzer case: double->byte->short
        short s5 = (short) c;            // char->short
        byte b3 = (byte) c;             // char->byte
        char c2 = (char) b;             // byte->char
        int i = 7;
        short s6 = (short) i;           // int->short
        byte b4 = b;                     // byte->byte no-op
    }
}
