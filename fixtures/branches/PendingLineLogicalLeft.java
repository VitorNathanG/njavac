// Regression: a static-false ungrouped negated shortcut used as a logical left
// operand loses its pending `if` position, while grouping that operand preserves
// it. A static-true left shortcut also carries the position through its evaluated
// right operand. njavac used to attach the false ungrouped line to the outer goto.
// Fuzzer-found Fuzz0622324.
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
