// Regression: goto-compaction must remove ONLY dead/redundant gotos, never a live one.
// Same nested constant short-circuit as GotoCompactDead but with an `else`, so the
// method has a REAL "skip the else" goto after the then-body. compact_gotos deletes
// the two spurious gotos (dead + goto-to-next) while KEEPING the skip-else goto: it is
// reachable (fall-through from the then) and its target is the merge, not the next
// instruction. This is the discriminator that the pass is not just "delete every goto".
public class GotoCompactKeepElse {
    public static void main(String[] args) {
        int v1 = 5;
        int vb = (byte) v1;
        if ((!((vb > 32766) || false)) || false) { v1++; } else { v1--; }
        v1++;
    }
}
