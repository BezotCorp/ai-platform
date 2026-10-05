use futures::io::{AsyncRead, AsyncWrite};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// A dialed, authorized client stream to a remote agent.
pub struct RoamingClientStream {
    pub agent_id: String,
    /// Kept alive so the connection isn't dropped while the stream is in use.
    pub conn: Connection,
    pub send: SendStream,
    pub recv: RecvStream,
}

impl RoamingClientStream {
    /// Consume the stream into `futures::io` read/write halves ready to feed to
    /// an ACP client transport (e.g. `ByteStreams::new(send, recv)`), plus the
    /// live [`Connection`] which the caller must keep alive for the duration of
    /// the session. This saves consumers from repeating the tokio-compat dance.
    pub fn into_futures_io(
        self,
    ) -> (
        impl AsyncWrite + Send + Unpin,
        impl AsyncRead + Send + Unpin,
        Connection,
    ) {
        (self.send.compat_write(), self.recv.compat(), self.conn)
    }
}
