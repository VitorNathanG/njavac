// Regression: active pending-line provenance becomes latent whenever it is used
// as an ungrouped logical left operand, regardless of its verdict or whether it
// came directly from a shortcut. Grouping after activation preserves it instead.
// Fuzzer-found Fuzz0667664.
public class PendingLinePositionState {
    public static void main(String[] args) {
        int x = 0;
        boolean p = x > 0;

        if (x == 0) {
            if (!(false && p) && false) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if ((!(false && p)) && false) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if (!((true || p) && false) && false) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if ((!((true || p) && false)) && false) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if (!(!(true || p) || true)) {
            }
        } else {
            x++;
        }
    }
}
