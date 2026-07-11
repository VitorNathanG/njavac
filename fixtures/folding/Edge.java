public class Edge {
    public static void main(String[] args) {
        int over = 1 << 33;
        int negshift = 1 << -1;
        int m1fold = ~0;
        int all = 0xFFFFFFFF;
        int and0 = 5 & 0;
        long lover = 1L << 65;
        long lm1 = ~0L;
        int foldToM1 = -1 | 0;
        int foldToNeg5 = ~4;
        System.out.println(over);
        System.out.println(negshift);
        System.out.println(m1fold);
        System.out.println(all);
        System.out.println(and0);
        System.out.println(lover);
        System.out.println(lm1);
        System.out.println(foldToM1);
        System.out.println(foldToNeg5);
    }
}
