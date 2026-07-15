// Regression: a static-false negated shortcut at the end of a then-arm emits no code,
// but its pending source line must attach to the outer skip-else goto. njavac used
// to omit that LineNumberTable entry because it recorded statement lines eagerly.
public class PendingLineGoto {
    public static void main(String[] args) {
        int x = 0;
        if (x == 0) {
            if (!(true || x > 0)) {
            }
        } else {
            x++;
        }
    }
}
