public class Prec {
    public static void main(String[] args) {
        int a = 1;
        int b = 2;
        int c = 3;
        int r1 = a | b & c;
        int r2 = a ^ b & c;
        int r3 = a | b ^ c;
        int r4 = a + b << c;
        int r5 = a << b + c;
        int r6 = a & b << c;
        System.out.println(r1);
        System.out.println(r2);
        System.out.println(r3);
        System.out.println(r4);
        System.out.println(r5);
        System.out.println(r6);
    }
}
