public class UnicodeEscapes {
    public static void main(String[] args) {
        char a = '\u0041';
        char b = '\u00ff';
        char z = '\u0020';
        char d = '\u7fff';
        char e = '\u8000';
        char f = '\uffff';
        System.out.println(a);
        System.out.println(b);
        System.out.println(z);
        System.out.println(d);
        System.out.println(e);
        System.out.println(f);
    }
}
