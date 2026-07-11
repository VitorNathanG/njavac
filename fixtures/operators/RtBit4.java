public class RtBit4 {
    public static void main(String[] args) {
        long v = 1024L;
        int amt = 3;
        long byInt = v << amt;
        long byLongShift = v >> amt;
        long byUnsigned = v >>> amt;
        long lamt = 5L;
        long byLong = v << lamt;
        long byLongR = v >> lamt;
        long byLongU = v >>> lamt;
        System.out.println(byInt);
        System.out.println(byLongShift);
        System.out.println(byUnsigned);
        System.out.println(byLong);
        System.out.println(byLongR);
        System.out.println(byLongU);
    }
}
