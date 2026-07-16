use super::ByteBuf;

/// Write one `CONSTANT_Utf8` payload. This extraction deliberately preserves the
/// current ordinary UTF-8 behavior; JVM modified UTF-8 lands in the next change.
pub(super) fn write(value: &str, buf: &mut ByteBuf) {
    let bytes = value.as_bytes();
    buf.u16(bytes.len() as u16);
    buf.bytes(bytes);
}
