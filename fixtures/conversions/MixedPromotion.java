public class MixedPromotion {
    public static void main(String[] args) {
        long L = 100L;
        int i = 2;
        double d = 5.0;
        float f = 3.0f;
        long r1 = L + i;
        long r2 = i + L;
        double r3 = d + i;
        float r4 = f + i;
        double r5 = L + d;
        float r6 = L + f;
        double r7 = f + d;
        System.out.println(r1);
        System.out.println(r2);
        System.out.println(r3);
        System.out.println(r4);
        System.out.println(r5);
        System.out.println(r6);
        System.out.println(r7);
    }
}
