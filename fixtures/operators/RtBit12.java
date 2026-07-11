public class RtBit12 {
    public static void main(String[] args) {
        int a = 100;
        int b = 7;
        int folded = (255 & 0xF0) | (3 << 4);
        int mixed = a & 0xFF | b << 2;
        System.out.println(folded);
        System.out.println(mixed);
    }
}
