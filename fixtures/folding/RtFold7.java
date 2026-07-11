public class RtFold7 {
    public static void main(String[] args) {
        int base = 10;
        int a = base + (2 * 3);
        int b = base * (100 % 7);
        int c = (1 << 4) + base;
        int d = base & (0xFF ^ 0x0F);
        int e = base | (6 & 3);
        int f = (~5) + base;
        long g = base + (1L << 40);
        double h = base + (0.5 + 0.5);
        int i = base - (-2147483648);
        int j = base * (2147483647 + 1);
        System.out.println(a);
        System.out.println(b);
        System.out.println(c);
        System.out.println(d);
        System.out.println(e);
        System.out.println(f);
        System.out.println(g);
        System.out.println(h);
        System.out.println(i);
        System.out.println(j);
    }
}
