public class LogicalCmpConst {
    public static void main(String[] args) {
        boolean q = false;
        int x = 0;
        if ((1 < 2) && q) { x = 1; }
        if ((2 < 1) || q) { x = 2; }
        if ((2 < 1) && q) { x = 3; }
        if ((1 < 2) || q) { x = 4; }
        System.out.println(x);
    }
}
