public class RtLitUnderscores {
    public static void main(String[] args) {
        int i = 1_234_567;
        long l = 1_000_000_000_000L;
        float f = 3_1.4_15f;
        double d = 3.141_592_653_589_793;
        int h = 0xDE_AD_BE_EF;
        int b = 0b0101_1010_0101_1010;
        System.out.println(i);
        System.out.println(l);
        System.out.println(f);
        System.out.println(d);
        System.out.println(h);
        System.out.println(b);
    }
}
