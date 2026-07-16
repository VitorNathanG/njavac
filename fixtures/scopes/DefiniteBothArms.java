// Assignment in both arms makes the local verifier-defined at the following join.
// Sema now supplies that joined state instead of codegen guessing from declarations.
public class DefiniteBothArms {
    public static void main(String[] args) {
        int seed = 1;
        int value;
        if (seed > 0) {
            value = 2;
        } else {
            value = 3;
        }
        if (value > 0) {
            System.out.println(value);
        }
    }
}
