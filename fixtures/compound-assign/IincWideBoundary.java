public class IincWideBoundary {
    public static void main(String[] args) {
        int x = 0;
        x += 32767;
        x += 32768;
        x -= 32768;
        x -= 32769;
    }
}
