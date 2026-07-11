public class Locals {
    public static void main(String[] args) {
        int a = 1;
        int b = 2;
        int c = 3;
        int d = a + b;
        int e = c * d;
        a = a + e;
        System.out.println(a);
        System.out.println(e);
    }
}
