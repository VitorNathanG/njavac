public class RtCmp11 {
    public static void main(String[] args) {
        short s = 1000;
        int y = 40000;
        s += y;
        s *= 2;
        s <<= 3;
        s >>= 1;
        s ^= 255;
        System.out.println(s);
    }
}
