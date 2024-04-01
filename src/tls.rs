use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use anyhow::{anyhow, Result};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::TlsAcceptor;

pub enum StreamType {
    Plain(TcpStream),
    Tls(tokio_rustls::server::TlsStream<TcpStream>),
}

impl StreamType {
    pub async fn upgrade_to_tls(self, tls_acceptor: TlsAcceptor) -> Result<Self> {
        match self {
            StreamType::Plain(stream) => {
                let tls_stream = tls_acceptor.accept(stream).await?;
                Ok(StreamType::Tls(tls_stream))
            }
            StreamType::Tls(_) => {
                tracing::warn!("Tried to update a tls stream, not going to do anything");
                Ok(self)
            }
        }
    }
    // pub async fn upgrade_to_tls_new(&mut self, tls_acceptor: &TlsAcceptor) -> Result<()> {
    //     let new_stream_type = match self {
    //         StreamType::Plain(stream) => {
    //             let tls_stream = tls_acceptor.accept(stream).await?;
    //             *self = StreamType::Tls(tls_stream);
    //             Ok(())
    //         }
    //         StreamType::Tls(_) => {
    //             tracing::warn!("Tried to update a tls stream, not going to do anything");
    //             Ok(())
    //         }
    //     };
    //     new_stream_type
    // }
}

impl AsyncWrite for StreamType {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::prelude::v1::Result<usize, std::io::Error>> {
        match self.get_mut() {
            StreamType::Tls(stream) => AsyncWrite::poll_write(std::pin::Pin::new(stream), cx, buf),
            StreamType::Plain(stream) => {
                AsyncWrite::poll_write(std::pin::Pin::new(stream), cx, buf)
            }
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::prelude::v1::Result<(), std::io::Error>> {
        match self.get_mut() {
            StreamType::Plain(stream) => AsyncWrite::poll_shutdown(std::pin::Pin::new(stream), cx),
            StreamType::Tls(stream) => AsyncWrite::poll_shutdown(std::pin::Pin::new(stream), cx),
        }
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::prelude::v1::Result<(), std::io::Error>> {
        match self.get_mut() {
            StreamType::Plain(stream) => AsyncWrite::poll_flush(std::pin::Pin::new(stream), cx),
            StreamType::Tls(stream) => AsyncWrite::poll_flush(std::pin::Pin::new(stream), cx),
        }
    }
}

impl AsyncRead for StreamType {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamType::Plain(stream) => AsyncRead::poll_read(std::pin::Pin::new(stream), cx, buf),
            StreamType::Tls(stream) => AsyncRead::poll_read(std::pin::Pin::new(stream), cx, buf),
        }
    }
}
