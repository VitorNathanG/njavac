// Regression: dead + redundant `goto` compaction (javac's Code.resolve).
// A nested constant-operand short-circuit like `(!(A || false)) || false` makes
// njavac's eager branch emitter leave behind two `goto`s that javac never keeps: one
// UNREACHABLE (nothing targets it once the conditional threads past it, and the goto
// before it has no fall-through) and one goto-to-NEXT that only shows up after the
// dead one is deleted and the pcs shift. njavac used to emit both, lengthening the
// Code attribute; compact_gotos deletes them in a >=2-round fixpoint. The surviving
// `if_icmpgt` must still target the MERGE (not the then-body): the deleted dead goto
// forwarded there, so its label had to thread through, not collapse onto the next
// byte. Fuzzer-found (Fuzz0000073). javac keeps the one append[int,int] merge frame.
public class GotoCompactDead {
    public static void main(String[] args) {
        int v1 = 5;
        int vb = (byte) v1;
        if ((!((vb > 32766) || false)) || false) { v1++; }
        v1++;
    }
}
