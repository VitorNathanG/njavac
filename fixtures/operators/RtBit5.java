public class RtBit5 {
    public static void main(String[] args) {
        int a = 5;
        long b = 5L;
        int ni = ~a;
        long nl = ~b;
        int ni2 = ~~a;
        System.out.println(ni);
        System.out.println(nl);
        System.out.println(ni2);
    }
}
