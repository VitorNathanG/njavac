public class RtLitLongRadix {
    public static void main(String[] args) {
        long dec = 4294967296L;
        long hex = 0x1_0000_0000L;
        long oct = 040000000000L;
        long bin = 0b1_00000000_00000000_00000000_00000000L;
        long low = 0xFFFF_FFFFL;
        System.out.println(dec);
        System.out.println(hex);
        System.out.println(oct);
        System.out.println(bin);
        System.out.println(low);
    }
}
