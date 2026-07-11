public class RtCmp15 {
    public static void main(String[] args) {
        byte b = 1;
        b <<= 10;
        short s = 1;
        s <<= 20;
        char c = 1;
        c <<= 17;
        b >>>= 1;
        s >>>= 1;
        System.out.println(b);
        System.out.println(s);
        System.out.println(c);
    }
}
