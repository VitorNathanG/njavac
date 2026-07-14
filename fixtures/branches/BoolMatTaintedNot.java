// Regression: a boolean VALUE that short-circuit-reduces to a single live bare leaf
// through a TAINTED `!` must materialize as the true/false DIAMOND (with its
// StackMapTable merge frame), not a bare `iload; istore`. njavac used to BARE it.
//
// A `!(inner)` is tainted when njavac's fold() collapses `inner` via a LEFT-constant
// short-circuit (fold(inner)=Some) while a live local is buried under it
// (contains_name(inner)) — e.g. `!(true || v1)`. gen_cond's fold-shortcut then
// erases the `!` before the Expr::Not => negate() arm can clear value_on_stack, and
// gen_bool_value's bare fast-path reused the loaded leaf — dropping the diamond's
// bytes + its frame (~13 bytes) that javac emits. Fix: a 6th fast-path conjunct
// `!taints_materialization(cond)` vetoes the bare reuse (see codegen.rs).
//
// The three-way lone-leaf split is the sharp core (a1 BARE / a2 DIAMOND / a3
// RESIDUAL): the axis is (fold(!-operand)=Some AND buried Name) vs (fold=None) vs
// (no !). Each row's shape + how it used to diverge is in its trailing comment.
public class BoolMatTaintedNot {
    public static void main(String[] args) {
        int x = 5;
        boolean v1 = x > 0;
        boolean v2 = x > 1;

        boolean a1 = (false && v1) || v1;             // BARE control (no !)                 — must not regress
        boolean a2 = (!(true || v1)) || v1;           // DIAMOND-3b (the bug)                — flips to diamond
        boolean a3 = (!((x > 0) && false)) || v1;     // RESIDUAL is_true (fold(inner)=None) — must not regress

        boolean g4 = ((!(true || v1)) && true) || v1; // DIAMOND: tainted ! on a nested surviving path
        boolean n3 = (true && (!(true || v1))) || v2; // DIAMOND: tainted ! inside a folding `true &&`
        boolean p3 = (!!!(true || v1)) || v2;         // DIAMOND: odd !-parity survives
        boolean p4 = (!!(true || v1)) && v2;          // DIAMOND: even parity, && reduces to v2
        boolean cc = (!((1 > 0) || v1)) || v1;        // DIAMOND: deciding const is a comparison
        boolean vb = (!(true || v1)) || (v1 & v2);    // DIAMOND: value-boolean & lone leaf
        boolean vx = (!(true || v1)) || (v1 ^ v2);    // DIAMOND: value-boolean ^ lone leaf

        boolean h1 = (false && (!(true || v1))) || v2; // BARE: tainting ! in a DROPPED branch — inert

        if ((!(true || v1)) || v1) {                  // if-context: byte-identical (pins the LNT no-op)
            System.out.println("t");
        }
    }
}
