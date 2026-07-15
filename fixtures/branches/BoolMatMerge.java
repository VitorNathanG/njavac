// Regression: a materialized boolean value that lands on a control-flow MERGE must
// use the true/false diamond, not njavac's reusable-stack fast path.
// `(a || true) && true` is statically true, but `a` is a real value-boolean whose
// short-circuit jump gets RESOLVED (a stack-map frame / merge) right before the final
// `&& v` loads v. So v sits on the stack at a merge point, and javac materializes it
// with `ifeq/iconst_1/goto/iconst_0` — NOT a bare load, even though the final CondItem
// is (ifne, reusable stack value, no chains), the same physical shape `true && p`
// leaves bare. Resolving the live left chain now marks the right CondItem as
// DiamondRequired directly; frame creation is only the resulting physical effect.
// njavac used to emit a bare `istore`, dropping the diamond's bytes. Fuzzer-found
// (Fuzz0002248). Complements NotLocalMat (the `!p`/`true && p` fast-path boundary).
public class BoolMatMerge {
    public static void main(String[] args) {
        int x = 5;
        boolean v1 = x > 0;
        boolean a = v1 & v1;                       // a value-boolean (bitwise), not a compare
        boolean r = ((a || true) && true) && v1;   // diamond: a's residual jump merges before v1
    }
}
