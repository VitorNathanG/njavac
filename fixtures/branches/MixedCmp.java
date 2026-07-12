public class MixedCmp {
    public static void main(String[] args) {
        int i = 3;
        long l = 7L;
        double d = 2.5;
        if (i < l) { System.out.println(1); }
        if (i < d) { System.out.println(2); }
        if (l > d) { System.out.println(3); }
    }
}
