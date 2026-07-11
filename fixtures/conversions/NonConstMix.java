public class NonConstMix {
    public static void main(String[] args) {
        int i = 7;
        long r1 = i + 2L;
        double r2 = i + 1.0;
        double r3 = (double)(i + 1);
        System.out.println(r1);
        System.out.println(r2);
        System.out.println(r3);
    }
}
