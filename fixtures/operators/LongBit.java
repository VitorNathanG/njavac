public class LongBit {
    public static void main(String[] args) {
        long a = 7L;
        long b = 3L;
        int s = 2;
        long and = a & b;
        long or = a | b;
        long xor = a ^ b;
        long not = ~a;
        long shl = a << s;
        long shr = a >> s;
        long ushr = a >>> s;
        long shlLong = a << b;
        System.out.println(and);
        System.out.println(or);
        System.out.println(xor);
        System.out.println(not);
        System.out.println(shl);
        System.out.println(shr);
        System.out.println(ushr);
        System.out.println(shlLong);
    }
}
