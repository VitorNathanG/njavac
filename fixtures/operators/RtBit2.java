public class RtBit2 {
    public static void main(String[] args) {
        int a = 1;
        int b = 33;
        int s1 = a << b;
        int s2 = 255 >> b;
        int s3 = a >>> b;
        int neg = -256;
        int s4 = neg >> a;
        int s5 = neg >>> a;
        System.out.println(s1);
        System.out.println(s2);
        System.out.println(s3);
        System.out.println(s4);
        System.out.println(s5);
    }
}
