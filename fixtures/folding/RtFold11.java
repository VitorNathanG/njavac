public class RtFold11 {
    public static void main(String[] args) {
        int a = (int) 3000000000L;
        int b = (int) 4294967296L;
        int c = (int) -4294967297L;
        short d = (short) 2147483647;
        byte e = (byte) 2147483647;
        char f = (char) 2147483647;
        int g = (int) (1L << 40);
        short h = (short) (1 << 20);
        byte i = (byte) (1L << 33);
        int j = (int) 9223372036854775807L;
        System.out.println(a);
        System.out.println(b);
        System.out.println(c);
        System.out.println(d);
        System.out.println(e);
        System.out.println(f);
        System.out.println(g);
        System.out.println(h);
        System.out.println(i);
        System.out.println(j);
    }
}
