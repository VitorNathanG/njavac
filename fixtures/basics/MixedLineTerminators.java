// Regression: LF, bare CR, and CRLF each count once; bare CR also ends `//`.
public class MixedLineTerminators {    public static void main(String[] args) {
        int cr = 1;        // A bare CR must end this comment.        int lf = 2;
        // CRLF must end this comment once.
        int crlf = 3;
        System.out.println(cr + lf + crlf);
    }
}
