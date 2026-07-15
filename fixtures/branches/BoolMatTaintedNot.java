// Regression: grouping a negated non-strict shortcut preserves a requirement to
// materialize a later bare leaf through the true/false DIAMOND (with its
// StackMapTable merge frame), not a bare `iload; istore`.
//
// `lowering_const` keeps `true || v1` structural because the complete subtree is
// not an immediate. The resulting Shortcut becomes NegatedShortcut under `!`;
// grouping upgrades its explicit materialization state before an outer logical
// expression selects a final leaf. The old fix reconstructed that history from the
// AST with has_tainted_not; CondItem now carries it directly.
//
// The three-way lone-leaf split remains the sharp core (a1 BARE / a2 DIAMOND / a3
// RESIDUAL). BoolGroupingProvenance adds the decisive grouped/unparenthesized and
// name-free controls that this older fixture did not distinguish.
public class BoolMatTaintedNot {
    public static void main(String[] args) {
        int x = 5;
        boolean v1 = x > 0;
        boolean v2 = x > 1;

        boolean a1 = (false && v1) || v1;             // BARE control (no !)                 — must not regress
        boolean a2 = (!(true || v1)) || v1;           // DIAMOND-3b (the bug)                — flips to diamond
        boolean a3 = (!((x > 0) && false)) || v1;     // RESIDUAL is_true (fold(inner)=None) — must not regress

        boolean g4 = ((!(true || v1)) && true) || v1; // DIAMOND: grouped negated shortcut, nested
        boolean n3 = (true && (!(true || v1))) || v2; // DIAMOND: grouped boundary under `true &&`
        boolean p3 = (!!!(true || v1)) || v2;         // DIAMOND: grouped triple negation
        boolean p4 = (!!(true || v1)) && v2;          // DIAMOND: grouped double negation
        boolean cc = (!((1 > 0) || v1)) || v1;        // DIAMOND: grouped comparison shortcut
        boolean vb = (!(true || v1)) || (v1 & v2);    // DIAMOND: value-boolean & lone leaf
        boolean vx = (!(true || v1)) || (v1 ^ v2);    // DIAMOND: value-boolean ^ lone leaf

        boolean h1 = (false && (!(true || v1))) || v2; // BARE: tainting ! in a DROPPED branch — inert

        if ((!(true || v1)) || v1) {                  // if-context: byte-identical (pins the LNT no-op)
            System.out.println("t");
        }
    }
}
