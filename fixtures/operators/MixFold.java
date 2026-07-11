public class MixFold {
    public static void main(String[] args) {
        int a = 42;
        int keep = a & 0xF;
        int shift = a << 3;
        int notLocal = ~a;
        int tree = a | 5 & 3;
        System.out.println(keep);
        System.out.println(shift);
        System.out.println(notLocal);
        System.out.println(tree);
    }
}
