// Regression: goto-compaction leaves a value-materialization diamond's own goto intact.
// The same nested constant short-circuit, but materialized into a boolean local rather
// than driving an `if`. javac builds the true/false diamond `iconst_1; goto M; iconst_0;
// M:` around it. compact_gotos removes the two spurious short-circuit gotos but must KEEP
// the diamond's `goto M` (reachable, and its target M is past `iconst_0`, not the next
// instruction). Guards against the pass over-reaching into materialization code.
public class GotoCompactDiamond {
    public static void main(String[] args) {
        int v1 = 5;
        int vb = (byte) v1;
        boolean r = (!((vb > 32766) || false)) || false;
        v1++;
    }
}
