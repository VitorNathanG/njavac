public class RtFold8 {
    public static void main(String[] args) {
        int a = 1 << 32;
        int b = 1 << 33;
        int c = 256 >> 33;
        int d = -1 >>> 32;
        long e = 1L << 64;
        long f = 1L << 65;
        long g = -1L >>> 64;
        int h = 16 >> 1;
        long i = 8L << 2;
        System.out.println(a);
        System.out.println(b);
        System.out.println(c);
        System.out.println(d);
        System.out.println(e);
        System.out.println(f);
        System.out.println(g);
        System.out.println(h);
        System.out.println(i);
    }
}
