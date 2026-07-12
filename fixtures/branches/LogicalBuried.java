public class LogicalBuried {
    public static void main(String[] args) {
        boolean a = false;
        boolean b = false;
        boolean r = false;
        int x = 0;
        if (a && (b && false)) { x = 1; }
        if ((a && false) || r) { x = 2; }
        System.out.println(x);
    }
}
