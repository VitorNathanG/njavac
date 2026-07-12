public class ElseIfChain {
    public static void main(String[] args) {
        int x = 5;
        if (x > 40) {
            x = 1;
        } else if (x > 30) {
            x = 2;
        } else if (x > 20) {
            x = 3;
        } else {
            x = 4;
        }
        System.out.println(x);
    }
}
