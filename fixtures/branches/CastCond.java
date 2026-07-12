public class CastCond {
    public static void main(String[] args) {
        double d = 4.5;
        if ((int) d > 3) { System.out.println(1); }
        long l = 10L;
        if ((int) l == 10) { System.out.println(2); }
        char c = 'A';
        if (c > 65) { System.out.println(3); }
        if (c == 'A') { System.out.println(4); }
    }
}
