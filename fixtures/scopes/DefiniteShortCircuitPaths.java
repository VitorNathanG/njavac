// Impossible short-circuit outcomes make dead operands vacuously definitely assigned.
// njavac previously rejected each read of the uninitialized boolean local.
public class DefiniteShortCircuitPaths {
    public static void main(String[] args) {
        boolean value;
        boolean runtime = true;
        if (false && value) {
            System.out.println(1);
        }
        if (true || value) {
            System.out.println(2);
        }
        if (runtime && false && value) {
            System.out.println(3);
        }
        if (runtime || true || value) {
            System.out.println(4);
        }
        if ((runtime || true) || value) {
            System.out.println(5);
        }
        if ((boolean) (runtime && false) && value) {
            System.out.println(6);
        }
        boolean andValue = false && value;
        boolean orValue = true || value;
        System.out.println(andValue);
        System.out.println(orValue);
    }
}
