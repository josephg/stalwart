/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    Command, ResponseType,
    protocol::compress,
    receiver::Request,
};

impl Request<Command> {
    pub fn parse_compress(self) -> trc::Result<compress::Arguments> {
        if self.tokens.len() == 1 {
            let algorithm = self.tokens.into_iter().next().unwrap().unwrap_bytes();
            if algorithm.eq_ignore_ascii_case(b"DEFLATE") {
                Ok(compress::Arguments { tag: self.tag })
            } else {
                Err(trc::ImapEvent::Error
                    .into_err()
                    .details(format!(
                        "Unsupported compression algorithm '{}'.",
                        String::from_utf8_lossy(&algorithm)
                    ))
                    .id(self.tag)
                    .ctx(trc::Key::Type, ResponseType::Bad))
            }
        } else {
            Err(self.into_error("Expected: COMPRESS <algorithm>."))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        protocol::compress,
        receiver::Receiver,
    };

    #[test]
    fn parse_compress_deflate() {
        let mut receiver = Receiver::new();

        // Valid DEFLATE (uppercase)
        assert_eq!(
            receiver
                .parse(&mut "t1 COMPRESS DEFLATE\r\n".as_bytes().iter())
                .unwrap()
                .parse_compress()
                .unwrap(),
            compress::Arguments {
                tag: "t1".into(),
            }
        );

        // Valid deflate (lowercase)
        assert_eq!(
            receiver
                .parse(&mut "t2 COMPRESS deflate\r\n".as_bytes().iter())
                .unwrap()
                .parse_compress()
                .unwrap(),
            compress::Arguments {
                tag: "t2".into(),
            }
        );

        // Invalid algorithm
        assert!(
            receiver
                .parse(&mut "t3 COMPRESS GZIP\r\n".as_bytes().iter())
                .unwrap()
                .parse_compress()
                .is_err()
        );

        // Missing argument
        assert!(
            receiver
                .parse(&mut "t4 COMPRESS\r\n".as_bytes().iter())
                .unwrap()
                .parse_compress()
                .is_err()
        );
    }
}
