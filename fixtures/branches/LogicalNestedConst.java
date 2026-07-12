public class LogicalNestedConst {
    public static void main(String[] args) {
        boolean a = true;
        boolean b = false;
        boolean c = true;
        if (a && false && b) { System.out.println(1); }
        if (a || true || b) { System.out.println(2); }
        if ((a && b) || false) { System.out.println(3); }
        if (a && (b || true)) { System.out.println(4); }
        if (a && b) { System.out.println(5); } else { System.out.println(6); }
        boolean r = a && b && c;
    }
}
