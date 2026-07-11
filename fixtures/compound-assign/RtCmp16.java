public class RtCmp16 {
    public static void main(String[] args) {
        long l = 10L;
        l += 2147483648L;
        l -= 100L;
        double d = 1.0;
        d += -2.5;
        d *= -4.0;
        float f = 8.0f;
        f -= -1.5f;
        f /= -2.0f;
        System.out.println(l);
        System.out.println(d);
        System.out.println(f);
    }
}
