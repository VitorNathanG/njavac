public class LogicalConstMat {
    public static void main(String[] args) {
        boolean q = false;
        boolean r1 = true && q;
        boolean r2 = q && false;
        boolean r3 = q || true;
        boolean r4 = false || q;
        boolean r5 = true || q;
        boolean r6 = false && q;
    }
}
