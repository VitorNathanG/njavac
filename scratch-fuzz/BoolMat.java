public class BoolMat {
    public static void main(String[] args) {
        int x = 5;
        boolean p = x > 3;
        boolean q = x < 9;
        boolean r1 = p;          // plain local
        boolean r2 = !p;         // NOT
        boolean r3 = !!p;        // double NOT
        boolean r4 = p & q;      // bitwise and
        boolean r5 = p | q;      // bitwise or
        boolean r6 = p ^ q;      // bitwise xor
        boolean r7 = true && p;  // && with const-true left
        boolean r8 = p && q;     // &&
        System.out.println(x);
    }
}
