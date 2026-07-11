public class ShiftAmt {
    public static void main(String[] args) {
        long v = 8L;
        int i = 2;
        long j = 3L;
        long byInt = v << i;
        long byLong = v << j;
        long byLongR = v >> j;
        long byLongU = v >>> j;
        System.out.println(byInt);
        System.out.println(byLong);
        System.out.println(byLongR);
        System.out.println(byLongU);
    }
}
