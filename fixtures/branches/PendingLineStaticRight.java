// Regression: a shortcut prefix followed by any code-free static right operand
// keeps latent position provenance. A later `!` must preserve the nested `if` line
// on the outer skip-else goto without changing value materialization.
// Fuzzer-found Fuzz0638056.
public class PendingLineStaticRight {
    public static void main(String[] args) {
        int x = 0;
        boolean p = x > 0;

        if (x == 0) {
            if (!((true || p) && true)) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if (!((false && p) || true)) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if (!!((true || p) && false)) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if (!!((false && p) || false)) {
            }
        } else {
            x++;
        }
    }
}
