public class RtBit11 {
    public static void main(String[] args) {
        long v = 65535L;
        int amt = 40;
        long r = v << amt;
        long a = 7L;
        long b = 2L;
        long shifted = a << b >> b << b;
        System.out.println(r);
        System.out.println(shifted);
    }
}
