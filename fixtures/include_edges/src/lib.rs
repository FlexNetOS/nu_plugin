pub const MESSAGE: &str = include_str!("../assets/message.txt");
pub const BLOB: &[u8] = include_bytes!("../assets/blob.bytes");

pub fn message_len() -> usize {
    MESSAGE.len() + BLOB.len()
}
