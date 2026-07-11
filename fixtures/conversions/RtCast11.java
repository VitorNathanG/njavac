public class RtCast11 {
    public static void main(String[] args) {
        double d = 9.9;
        long chained = (long) (float) d;
        int narrow = (short) (int) d;
        System.out.println(chained);
        System.out.println(narrow);
    }
}
