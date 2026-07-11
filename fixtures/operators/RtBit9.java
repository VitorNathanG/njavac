public class RtBit9 {
    public static void main(String[] args) {
        long a = 3735928559L;
        long b = 15L;
        long c = a & b;
        long d = a | b;
        long e = a ^ b;
        long f = ~a & b;
        System.out.println(c);
        System.out.println(d);
        System.out.println(e);
        System.out.println(f);
    }
}
