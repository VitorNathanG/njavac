public class NotCond {
    public static void main(String[] args) {
        int a = 3;
        if (!(a < 5)) { System.out.println(1); }
        boolean r = !(a < 5);
        boolean b = true;
        if (!b) { System.out.println(2); }
        System.out.println(r);
    }
}
