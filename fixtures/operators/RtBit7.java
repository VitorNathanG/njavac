public class RtBit7 {
    public static void main(String[] args) {
        int a = 240;
        int b = 15;
        int c = 2;
        int d = 1;
        int r = a & b << c | d;
        int r2 = a + b & c;
        int r3 = a & b + c;
        System.out.println(r);
        System.out.println(r2);
        System.out.println(r3);
    }
}
