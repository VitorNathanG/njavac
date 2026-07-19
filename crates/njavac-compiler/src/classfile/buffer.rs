/// Big-endian byte buffer.
pub(super) struct ByteBuf(Vec<u8>);

impl ByteBuf {
    pub(super) fn with_capacity(n: usize) -> Self {
        ByteBuf(Vec::with_capacity(n))
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    pub(super) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }

    pub(super) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }

    pub(super) fn reserve_u32(&mut self) -> usize {
        let offset = self.len();
        self.u32(0);
        offset
    }

    pub(super) fn patch_u32(&mut self, offset: usize, v: u32) {
        self.0[offset..offset + 4].copy_from_slice(&v.to_be_bytes());
    }

    pub(super) fn bytes(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }

    pub(super) fn into_vec(self) -> Vec<u8> {
        self.0
    }
}
