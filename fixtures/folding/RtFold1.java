public class RtFold1 {
    public static void main(String[] args) {
        int a = 2147483647 + 1;
        int b = 2147483647 * 2;
        int c = -2147483648 - 1;
        int d = 2147483647 + 2147483647;
        int e = 1000000 * 1000000;
        int f = -2147483648 / -1;
        System.out.println(a);
        System.out.println(b);
        System.out.println(c);
        System.out.println(d);
        System.out.println(e);
        System.out.println(f);
    }
}
