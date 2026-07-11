public class RtCast2 {
    public static void main(String[] args) {
        long l = 4000000000L;
        int i = (int) l;
        float f = (float) l;
        double d = (double) l;
        System.out.println(i);
        System.out.println(f);
        System.out.println(d);
    }
}
