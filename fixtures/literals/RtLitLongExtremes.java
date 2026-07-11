public class RtLitLongExtremes {
    public static void main(String[] args) {
        long max = 9223372036854775807L;
        long min = -9223372036854775808L;
        long maxHex = 0x7FFF_FFFF_FFFF_FFFFL;
        long negOne = 0xFFFF_FFFF_FFFF_FFFFL;
        long big = 5_000_000_000L;
        long lo = 3L;
        System.out.println(max);
        System.out.println(min);
        System.out.println(maxHex);
        System.out.println(negOne);
        System.out.println(big);
        System.out.println(lo);
    }
}
