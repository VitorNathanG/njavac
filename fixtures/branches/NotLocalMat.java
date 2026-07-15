// Regression: materializing the negation of a boolean *local*.
// A bare boolean local loaded for a condition sits on the stack as 0/1 with a
// pending `ifne` — njavac's `stack_reuse` fast-path reuses it directly instead
// of building the true/false diamond. But `!p` INVERTS the result while the loaded
// bits stay `p`, so reusing them miscompiled `boolean r = !p` to `r = p` (a real
// wrong-answer bug, not just a byte difference). Fix: `negate()` clears the flag, so
// `!p` and `!!p` both go through the diamond, exactly as javac does (javac diamonds
// every negation). `true && p` is the boundary: it stays a bare `iload` because its
// value came from the un-negated right operand, so the fast-path still fires there.
// Fuzzer-found (Fuzz0000356). Complements NotCond (which negates a *comparison*, not
// a local, so it never hit the fast-path).
public class NotLocalMat {
    public static void main(String[] args) {
        int x = 5;
        boolean p = x > 3;       // boolean local from a comparison
        boolean a = !p;          // diamond: iload; ifne; iconst_1; goto; iconst_0
        boolean b = !!p;         // diamond too: javac does NOT collapse to identity
        boolean c = true && p;   // boundary: bare iload (fast-path still fires)
        boolean d = p;           // plain copy: bare iload (never routed to the diamond)
        System.out.println(a);
        System.out.println(b);
        System.out.println(c);
        System.out.println(d);
    }
}
