// Regression: a String NUL must use the two-byte modified UTF-8 encoding, not a raw zero byte.
public class ModifiedUtf8Nul {
    public static void main(String[] args) {
        System.out.println("\0");
    }
}
