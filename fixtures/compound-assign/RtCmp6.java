public class RtCmp6 {
    public static void main(String[] args) {
        long x = 65536L;
        int c = 3;
        x <<= c;
        x >>= 1;
        x >>>= c;
        x <<= 65;
        x >>>= 70;
        System.out.println(x);
    }
}
