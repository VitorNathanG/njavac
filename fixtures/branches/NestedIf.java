public class NestedIf {
    public static void main(String[] args) {
        int x = 5;
        int y = 6;
        if (x > 3) {
            if (y > 4) {
                x = 10;
            }
        }
        System.out.println(x);
    }
}
