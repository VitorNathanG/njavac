public class RtCmp14 {
    public static void main(String[] args) {
        int x = 7;
        int y = 3;
        int z = 0;
        z += x + y;
        z -= x * y;
        z *= x - y;
        z |= x & y;
        z ^= x | y;
        System.out.println(z);
    }
}
