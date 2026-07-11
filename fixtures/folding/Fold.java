public class Fold {
    public static void main(String[] args) {
        int not5 = ~5;
        int and = 6 & 3;
        int shl = 1 << 4;
        int ushr = 0xFF >>> 2;
        int or = 0x10 | 0x01;
        int xor = 0xFF ^ 0x0F;
        int shr = -16 >> 2;
        int big = 1 << 30;
        long lshl40 = 1L << 40;
        long land = 0xFFFFFFFFL & 0xFF00L;
        long lnot = ~7L;
        System.out.println(not5);
        System.out.println(and);
        System.out.println(shl);
        System.out.println(ushr);
        System.out.println(or);
        System.out.println(xor);
        System.out.println(shr);
        System.out.println(big);
        System.out.println(lshl40);
        System.out.println(land);
        System.out.println(lnot);
    }
}
