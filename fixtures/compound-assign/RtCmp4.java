public class RtCmp4 {
    public static void main(String[] args) {
        int a = 5;
        int x = 1024;
        x <<= a;
        x >>= a;
        x >>>= a;
        x <<= 33;
        x >>= 1;
        x >>>= 40;
        System.out.println(x);
    }
}
