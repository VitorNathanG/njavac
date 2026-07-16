use super::ByteBuf;

/// Write one `CONSTANT_Utf8` payload from Java's UTF-16 code units. Modified UTF-8
/// encodes NUL as `c0 80` and encodes each surrogate separately, so a supplementary
/// scalar occupies six bytes rather than standard UTF-8's four.
pub(super) fn write(value: &str, buf: &mut ByteBuf) {
    let encoded_len: usize = value
        .encode_utf16()
        .map(|unit| match unit {
            0 => 2,
            1..=0x7f => 1,
            0x80..=0x7ff => 2,
            _ => 3,
        })
        .sum();
    let encoded_len =
        u16::try_from(encoded_len).expect("modified UTF-8 payload exceeds classfile limit");
    buf.u16(encoded_len);

    for unit in value.encode_utf16() {
        match unit {
            0 => {
                buf.u8(0xc0);
                buf.u8(0x80);
            }
            1..=0x7f => buf.u8(unit as u8),
            0x80..=0x7ff => {
                buf.u8(0xc0 | (unit >> 6) as u8);
                buf.u8(0x80 | (unit & 0x3f) as u8);
            }
            _ => {
                buf.u8(0xe0 | (unit >> 12) as u8);
                buf.u8(0x80 | ((unit >> 6) & 0x3f) as u8);
                buf.u8(0x80 | (unit & 0x3f) as u8);
            }
        }
    }
}
