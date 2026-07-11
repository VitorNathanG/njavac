public class IntLiteralForms {
    public static void main(String[] args) {
        int hex = 0xCAFE;
        int hexNeg = 0xFFFFFFFF;
        int oct = 0777;
        int bin = 0b1010;
        int under = 1_000_000;
        int underHex = 0x00_FF;
        System.out.println(hex);
        System.out.println(hexNeg);
        System.out.println(oct);
        System.out.println(bin);
        System.out.println(under);
        System.out.println(underHex);
    }
}
