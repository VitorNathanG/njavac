public class NestedThenElse {
    public static void main(String[] args) {
        int x = 5;
        if (x > 3) {
            if (x > 4) { x = 1; } else { x = 2; }
        } else {
            x = 3;
        }
        System.out.println(x);
    }
}
