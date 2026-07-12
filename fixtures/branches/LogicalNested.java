public class LogicalNested {
    public static void main(String[] args) {
        boolean a = true;
        boolean b = false;
        boolean c = true;
        int x = 5;
        if (a || b || c) { System.out.println(1); }
        if (a && b || c) { System.out.println(2); }
        if (a || b && c) { System.out.println(3); }
        if (!(a && b)) { System.out.println(4); }
        if (x < 3 && a || !b) { System.out.println(5); }
    }
}
