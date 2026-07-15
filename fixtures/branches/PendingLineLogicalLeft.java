// Regression: an active pending position used as an ungrouped logical left operand
// becomes latent, while grouping preserves it through the wrapper. A later `!`
// can reactivate the latent state. njavac used to attach the ungrouped false line
// directly to the outer goto. Fuzzer-found Fuzz0622324.
public class PendingLineLogicalLeft {
    public static void main(String[] args) {
        int x = 0;
        boolean p = x > 0;

        if (x == 0) {
            if (!(true || p) || false) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if ((!(true || p)) || false) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if (!(!(false && p) && true)) {
            }
        } else {
            x++;
        }
    }
}
