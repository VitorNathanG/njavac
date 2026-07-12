public class LogicalConstIf {
    public static void main(String[] args) {
        boolean q = false;
        if (true && q) { System.out.println(1); }
        if (false || q) { System.out.println(2); }
        if (q && true) { System.out.println(3); }
        if (q || false) { System.out.println(4); }
        if (true || q) { System.out.println(5); }
        if (false && q) { System.out.println(6); }
    }
}
