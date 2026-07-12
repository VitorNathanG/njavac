public class CompoundInIf {
    public static void main(String[] args) {
        int x = 5;
        if (x > 3) {
            x += 5;
            x *= 2;
        } else {
            x -= 1;
            x++;
        }
        System.out.println(x);
    }
}
