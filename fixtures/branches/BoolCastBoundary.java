// Regression: a boolean cast consumes its operand as a value before an enclosing
// logical/if consumer. A deciding shortcut therefore emits iconst_0/1 and a real
// outer test; repeated casts reuse the first materialized value. njavac previously
// folded through the cast and lost those branches, frames, and sometimes an arm.
public class BoolCastBoundary {
    public static void main(String[] args) {
        int x = 1;
        boolean p = x > 0;
        boolean q = x > 1;

        boolean d1 = (boolean) p;                              // bare
        boolean d2 = (boolean) (true || p);                    // iconst_1
        boolean c1 = ((boolean) (true || p)) && q;             // cast value then outer test
        boolean c2 = ((boolean) (false && p)) || q;            // opposite verdict
        boolean c3 = ((boolean) (!(true || p))) || q;          // negated shortcut
        boolean c4 = ((boolean) ((!(true || p)))) || q;        // grouping then cast
        boolean c5 = ((boolean) (boolean) (true || p)) && q;   // repeated cast
        boolean c6 = ((boolean) !p) && q;                      // live negation
        boolean c7 = ((boolean) !!p) && q;                     // repeated negation

        if ((boolean) (false && p)) {
            x++;
        } else {
            x--;
        }
    }
}
