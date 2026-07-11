public class ExplicitCasts {
    public static void main(String[] args) {
        int i = 2;
        long L = 3L;
        float f = 4.0f;
        double d = 5.0;
        long a = (long)i;
        float b = (float)i;
        double c = (double)i;
        int e = (int)L;
        float g = (float)L;
        double h = (double)L;
        int j = (int)f;
        long k = (long)f;
        double m = (double)f;
        int n = (int)d;
        long o = (long)d;
        float p = (float)d;
        int q = (byte)i;
        int r = (char)i;
        int s = (short)i;
        System.out.println(a);
        System.out.println(e);
        System.out.println(k);
        System.out.println(q);
    }
}
