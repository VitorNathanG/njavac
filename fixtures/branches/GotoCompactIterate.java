// Regression: goto-compaction must iterate to a true fixpoint, not stop after one pass.
// A deeper `(((!(A || false)) || false) || false) || false` chain stacks up several
// spurious gotos where each deletion (dead, then goto-to-next) only exposes the next
// one after the pcs shift. Deleting them needs MORE than the two rounds GotoCompactDead
// exercises, so this pins that the delete->relayout loop runs until no goto is dead or
// goto-to-next, matching javac's single `if_icmpgt` to the merge.
public class GotoCompactIterate {
    public static void main(String[] args) {
        int v1 = 5;
        int vb = (byte) v1;
        if ((((!((vb > 32766) || false)) || false) || false) || false) { v1++; }
        v1++;
    }
}
