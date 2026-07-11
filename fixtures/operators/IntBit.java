public class IntBit {
    public static void main(String[] args) {
        int a = 12;
        int b = 5;
        int and = a & b;
        int or = a | b;
        int xor = a ^ b;
        int not = ~a;
        int shl = a << b;
        int shr = a >> b;
        int ushr = a >>> b;
        System.out.println(and);
        System.out.println(or);
        System.out.println(xor);
        System.out.println(not);
        System.out.println(shl);
        System.out.println(shr);
        System.out.println(ushr);
    }
}
