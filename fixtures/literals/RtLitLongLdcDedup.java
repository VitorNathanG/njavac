public class RtLitLongLdcDedup {
    public static void main(String[] args) {
        long dec = 4294967296L;
        long hex = 0x1_0000_0000L;
        long a = 2L;
        long b = 2L;
        long same1 = 123456789012L;
        long same2 = 123456789012L;
        System.out.println(dec);
        System.out.println(hex);
        System.out.println(a);
        System.out.println(b);
        System.out.println(same1);
        System.out.println(same2);
    }
}
