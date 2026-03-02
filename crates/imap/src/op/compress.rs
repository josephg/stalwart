/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::time::Instant;

use crate::core::{Session, compress::DeflateDecompressor};
use common::listener::SessionStream;
use directory::Permission;
use imap_proto::{
    Command, ResponseCode, StatusResponse,
    receiver::Request,
};

impl<T: SessionStream> Session<T> {
    pub async fn handle_compress(&mut self, request: Request<Command>) -> trc::Result<()> {
        // Validate access
        self.assert_has_permission(Permission::ImapCompress)?;

        let op_start = Instant::now();

        // Check if compression is already active
        if self.is_compress {
            return self.write_bytes(
                StatusResponse::no("DEFLATE compression is already active.")
                    .with_code(ResponseCode::CompressionActive)
                    .with_tag(request.tag)
                    .into_bytes(),
            )
            .await;
        }

        // Parse the command (validates DEFLATE argument)
        let arguments = request.parse_compress()?;

        trc::event!(
            Imap(trc::ImapEvent::Compress),
            SpanId = self.session_id,
            Elapsed = op_start.elapsed()
        );

        // Send the OK response UNCOMPRESSED (per RFC 4978: compression starts
        // immediately after the CRLF ending the tagged OK response)
        self.write_bytes(
            StatusResponse::ok("DEFLATE active")
                .with_tag(arguments.tag)
                .into_bytes(),
        )
        .await?;

        // Enable compression on both directions
        self.is_compress = true;
        self.decompressor = Some(DeflateDecompressor::new());
        {
            let mut writer = self.stream_tx.lock().await;
            writer.enable_compression();
        }

        Ok(())
    }
}
