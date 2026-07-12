public class DeadBranch {
    public static void main(String[] args) {
        int x = 5;
        if (true) { x = 1; } else { x = 2; }
        if (false) { x = 3; } else { x = 4; }
        if (false) { x = 5; }
        if (1 < 2) { x = 6; }
        if (2 < 1) { x = 7; }
        boolean c = 3 == 3;
        System.out.println(x);
    }
}
