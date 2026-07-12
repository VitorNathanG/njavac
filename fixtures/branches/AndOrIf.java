public class AndOrIf {
    public static void main(String[] args) {
        int a = 1;
        int b = 2;
        int c = 3;
        int d = 4;
        if (a < b && c < d) { System.out.println(1); }
        if (a < b || c < d) { System.out.println(2); }
    }
}
