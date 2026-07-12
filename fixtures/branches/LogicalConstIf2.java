public class LogicalConstIf2 {
    public static void main(String[] args) {
        boolean q = false;
        if (q && false) { System.out.println(1); }
        if (q || true) { System.out.println(2); }
        System.out.println(9);
    }
}
