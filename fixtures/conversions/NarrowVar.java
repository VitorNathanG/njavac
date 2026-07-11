public class NarrowVar {
    public static void main(String[] args) {
        int x = 300;
        byte a = (byte) x;
        System.out.println(a);
        short b = (short) x;
        System.out.println(b);
        char c = (char) x;
        System.out.println(c);
    }
}
