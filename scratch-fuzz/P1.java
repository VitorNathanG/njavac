public class P1 {
    public static void main(String[] args) {
        int v1 = 5;
        int vb = (byte) v1;
        if (!(vb > 32766)) {
            v1++;
        }
        v1++;
    }
}
