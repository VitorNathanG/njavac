public class RtBit10 {
    public static void main(String[] args) {
        int x = 1;
        int y = 2;
        x &= y;
        x |= y;
        x ^= y;
        x <<= y;
        x >>= y;
        x >>>= y;
        System.out.println(x);
        long a = 1024L;
        long b = 3L;
        a &= b;
        a |= b;
        a ^= b;
        a <<= b;
        a >>= b;
        a >>>= b;
        System.out.println(a);
    }
}
