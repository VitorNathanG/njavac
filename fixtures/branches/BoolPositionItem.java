// Regression: pending source positions are independent from grouping and value
// materialization. A code-free false negated shortcut at the end of an outer then
// arm attaches its line to the outer skip-else goto for ungrouped, grouped, and
// name-free forms; a following statement overwrites the pending line.
public class BoolPositionItem {
    public static void main(String[] args) {
        int x = 0;
        boolean p = x > 0;

        if (x == 0) {
            if (!(true || p)) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if ((!(true || p))) {
            }
        } else {
            x++;
        }

        if (x == 0) {
            if (!(true || (1L >>> 1L) > 0L)) {
            }
        } else {
            x++;
        }

        if ((!(true || p))) {
        }
        x++;
    }
}
