//! Drives framed tunnel I/O and dispatches commands and server responses.
mod command;
mod driver;

pub(super) use command::Command;
use driver::Driver;

use crate::{StreamLimits, protocol::decode_frame};
use bsobfs::the_fourth_utils::futures_util::{SinkExt, StreamExt, stream::SplitStream};
use std::io;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_util::{
    bytes::{Bytes, BytesMut},
    codec::{Decoder, Encoder, Framed},
};

pub(super) async fn run<C, T>(
    stream: Framed<T, C>,
    commands: mpsc::Receiver<Command>,
    closes: mpsc::UnboundedReceiver<u64>,
    limits: StreamLimits,
) -> io::Result<()>
where
    C: Decoder<Item = BytesMut, Error = io::Error> + Encoder<Bytes, Error = io::Error> + Unpin,
    T: AsyncRead + AsyncWrite + Unpin,
{
    let (mut sink, reader) = stream.split();
    let (outgoing, mut output) = mpsc::channel::<Bytes>(limits.sink_queue_depth.max(1));
    let mut driver = Driver::new(outgoing);
    let write = async {
        while let Some(frame) = output.recv().await {
            sink.send(frame).await?;
        }
        sink.close().await
    };
    let dispatch = driver.dispatch(commands, closes, reader);
    let result = tokio::try_join!(dispatch, write).map(|_| ());
    driver.finish(&result);
    result
}

impl Driver {
    async fn dispatch<C, T>(
        &mut self,
        mut commands: mpsc::Receiver<Command>,
        mut closes: mpsc::UnboundedReceiver<u64>,
        mut reader: SplitStream<Framed<T, C>>,
    ) -> io::Result<()>
    where
        C: Decoder<Item = BytesMut, Error = io::Error> + Encoder<Bytes, Error = io::Error> + Unpin,
        T: AsyncRead + AsyncWrite + Unpin,
    {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break; };
                    if self.command(command).await? { break; }
                }
                Some(id) = closes.recv() => self.close(id).await?,
                frame = reader.next() => {
                    let frame = frame.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "forward server disconnected"))??;
                    let (action, payload) = decode_frame(frame).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid forwarding frame")
                    })?;
                    self.received(action, payload).await?;
                }
            }
        }
        // Finish the tunnel write side, then read until the server has drained
        // its queued replies. Keeping the reader alive prevents a reset during
        // normal shutdown while Close acknowledgements are still in flight.
        self.outgoing.take();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while let Some(frame) = reader.next().await {
                frame?;
            }
            Ok::<(), io::Error>(())
        })
        .await?
    }
}
