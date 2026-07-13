public class NotNot {
    public static void main(String[] args) {
        int x = 5;
        boolean v1 = x > 3;
        boolean a = !v1;      // diamond (negated)
        boolean b = !!v1;     // no diamond (double-negate restores identity)
        boolean c = v1;       // no diamond (plain copy)
        System.out.println(x);
    }
}
