// Regression: a code-free false nested `if` that begins at a live branch target
// must not attach its line to the enclosing skip-else goto. javac clears the
// pending position at that control entry; njavac used to preserve it as though the
// statement had been reached by straight-line execution. Fuzzer-found Fuzz0154411.
public class PendingLineAtJoin {
    public static void main(String[] args) {
        int x = 0;
        boolean p = x > 0;
        if (x == 0) {
            if (x == 1) {
                x++;
            }
            if (!(true || p)) {
            }
        } else {
            x--;
        }
    }
}
