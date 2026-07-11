public class RtFold2 {
    public static void main(String[] args) {
        long a = 9223372036854775807L + 1L;
        long b = 9223372036854775807L * 2L;
        long c = -9223372036854775808L - 1L;
        long d = 9223372036854775807L + 9223372036854775807L;
        long e = 3037000500L * 3037000500L;
        long f = -9223372036854775808L / -1L;
        System.out.println(a);
        System.out.println(b);
        System.out.println(c);
        System.out.println(d);
        System.out.println(e);
        System.out.println(f);
    }
}
