// An unassigned interior slot must be Top when a later assigned local reaches a frame.
// Codegen previously marked every declaration assigned in its parallel locals model.
public class DefiniteTopHole {
    public static void main(String[] args) {
        int seed = 1;
        int unused;
        int value = 2;
        if (seed > 0) {
            System.out.println(value);
        }
    }
}
