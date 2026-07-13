public class P2 {
    public static void main(String[] args) {
        int v1 = 5;
        int vb = (byte) v1;
        if ((vb > 32766) || false) {
            v1++;
        }
        v1++;
    }
}
