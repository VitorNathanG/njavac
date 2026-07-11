public class IincByteBoundary {
    public static void main(String[] args) {
        int x = 0;
        x += 127;
        x += 128;
        x -= 128;
        x -= 129;
    }
}
