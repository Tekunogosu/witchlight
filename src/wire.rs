//! What crosses the API channel that is not JSON's own: bytes as base64.
//!
//! Plain base64, the alphabet .NET's `Convert.ToBase64String` writes, because
//! the other end of every wire here is a C# serializer. Hand-rolled rather than
//! a crate: it is thirty lines, and a dependency for thirty lines is a
//! dependency somebody has to audit.

/// Bytes out of base64 text, or why not.
pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [255u8; 256];
    for (i, &b) in ALPHABET.iter().enumerate() {
        table[b as usize] = i as u8;
    }

    let stripped: Vec<u8> = text.bytes().filter(|&b| b != b'=' && !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(stripped.len() * 3 / 4);
    for chunk in stripped.chunks(4) {
        let mut buf = [0u8; 4];
        for (i, &b) in chunk.iter().enumerate() {
            let value = table[b as usize];
            if value == 255 {
                return Err("invalid base64".to_owned());
            }
            buf[i] = value;
        }
        let n = chunk.len();
        if n == 1 {
            return Err("a base64 group of one character".to_owned());
        }
        out.push((buf[0] << 2) | (buf[1] >> 4));
        if n > 2 {
            out.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if n > 3 {
            out.push((buf[2] << 6) | buf[3]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_encoded_the_way_dotnet_would_decode_back() {
        // "AQIDBAUG" is base64 for bytes 1..=6, the shape .NET's own encoder
        // would produce for one six-byte entry with no padding needed.
        assert_eq!(decode("AQIDBAUG").expect("valid base64"), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn padding_decodes_to_the_right_length() {
        // "AQI=" is bytes [1, 2], padded to a multiple of four characters.
        assert_eq!(decode("AQI=").expect("valid base64"), vec![1, 2]);
        assert_eq!(decode("AQ==").expect("valid base64"), vec![1]);
    }

    #[test]
    fn what_is_not_base64_is_refused() {
        assert!(decode("A").is_err());
        assert!(decode("A!==").is_err());
    }
}
