public class RtNest11 {
    public static void main(String[] args) {
        int i = 3;
        long L = 8L;
        float f = 1.25f;
        double d = 4.0;
        double r = L + i - f * (L + d) / i;
        System.out.println(r);
    }
}
