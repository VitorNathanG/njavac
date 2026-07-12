public class LogicalConstElse {
    public static void main(String[] args) {
        boolean q = false;
        int x = 0;
        if (q && false) { x = 1; } else { x = 2; }
        if (q || true)  { x = 3; } else { x = 4; }
        System.out.println(x);
    }
}
