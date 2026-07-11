public class RtCmp1 {
    public static void main(String[] args) {
        int x = 0;
        x += 1;
        x += 127;
        x += 128;
        x += 32767;
        x += 32768;
        x -= 1;
        x -= 128;
        x -= 129;
        System.out.println(x);
    }
}
