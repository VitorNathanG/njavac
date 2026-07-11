public class RtNest4 {
    public static void main(String[] args) {
        int i = 10;
        long L = 3L;
        float f = 1.5f;
        double d = 2.0;
        double r = i + (L - (f + (i * d)));
        System.out.println(r);
    }
}
