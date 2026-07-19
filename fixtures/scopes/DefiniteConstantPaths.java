// Constant condition outcomes make only the normally reachable arm govern definite assignment.
// njavac previously rejected the reads after these single reachable assignments and in dead arms.
public class DefiniteConstantPaths {
    public static void main(String[] args) {
        int literal;
        if (true) {
            literal = 1;
        }
        System.out.println(literal);

        int comparison;
        if (1 < 2) {
            comparison = 2;
        }
        System.out.println(comparison);

        int wrapped;
        if (!((boolean) false)) {
            wrapped = 3;
        }
        System.out.println(wrapped);

        int deadRead;
        if (false) {
            System.out.println(deadRead);
        }
    }
}
