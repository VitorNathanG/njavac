// Regression: grouping the complete negation of a non-strict shortcut preserves
// javac's requirement to diamond-materialize a later boolean value. The same
// expression without that grouping stays bare. njavac previously erased the
// parentheses, over-materialized the ungrouped forms, and missed the name-free
// long>>>long form because its fallback looked only for local names.
public class BoolGroupingProvenance {
    public static void main(String[] args) {
        int x = 1;
        boolean p = x > 0;
        boolean q = x > 1;

        boolean u1 = !(true || p) || q;                         // bare q
        boolean g1 = (!(true || p)) || q;                       // diamond q
        boolean u2 = !!(true || p) && q;                        // bare q
        boolean g2 = (!!(true || p)) && q;                      // diamond q
        boolean u3 = !!!(true || p) || q;                       // bare q
        boolean g3 = (!!!(true || p)) || q;                     // diamond q

        boolean nf = (!(true || (1L >>> 1L) > 0L)) || q;       // name-free bug
        boolean sf = (!(true || (1L >> 1L) > 0L)) || q;        // strict-fold control
        boolean op = (!(false && (1L >>> 1L) > 0L)) && q;      // opposite verdict
        boolean vb = (!(true || (1L >>> 1L) > 0L)) || (p & q); // value-boolean leaf
        boolean dd = true || ((!(true || p)) || q);            // grouped dead operand
    }
}
