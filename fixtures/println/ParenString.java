// Regression: parenthesized String literals must retain their reference type in value codegen.
public class ParenString {
    public static void main(String[] args) {
        System.out.println(("text"));
    }
}
