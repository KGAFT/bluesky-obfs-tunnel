//! One-shot payload transfer over fixed or multiplexed tunnel connections.
use bluesky_obfs_tunnel::{
    client::{ForwardConnection, ForwardProtocol},
    protocol::MAX_TUNNEL_CHUNK,
};
use bsobfs::the_fourth_utils::futures_util::{SinkExt, StreamExt};
use std::io;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::{
    bytes::{Bytes, BytesMut},
    codec::{Decoder, Encoder, Framed},
};

pub(super) async fn raw_tcp<C, T, W>(
    stream: Framed<T, C>,
    payload: Bytes,
    output: &mut W,
) -> io::Result<()>
where
    C: Decoder<Item = BytesMut, Error = io::Error> + Encoder<Bytes, Error = io::Error> + Unpin,
    T: AsyncRead + AsyncWrite + Unpin,
    W: AsyncWrite + Unpin,
{
    let (mut writer, mut reader) = stream.split();
    let send = async {
        for chunk in payload.chunks(MAX_TUNNEL_CHUNK) {
            writer.send(Bytes::copy_from_slice(chunk)).await?;
        }
        writer.close().await
    };
    let receive = async {
        while let Some(frame) = reader.next().await {
            output.write_all(&frame?).await?;
        }
        output.flush().await
    };
    tokio::try_join!(send, receive)?;
    Ok(())
}

pub(super) async fn multiplexed<W: AsyncWrite + Unpin>(
    connection: ForwardConnection,
    payload: Bytes,
    output: &mut W,
) -> io::Result<()> {
    let protocol = connection.protocol();
    let (sender, mut receiver) = connection.split();
    let send = async {
        sender.send(payload).await?;
        if protocol == ForwardProtocol::Tcp {
            sender.shutdown_write().await?;
        }
        Ok::<_, io::Error>(())
    };
    let receive = async {
        loop {
            match receiver.recv().await? {
                Some(data) => output.write_all(&data).await?,
                None if protocol == ForwardProtocol::Udp => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "UDP endpoint closed before replying",
                    ));
                }
                None => break,
            }
            if protocol == ForwardProtocol::Udp {
                break;
            }
        }
        output.flush().await
    };
    tokio::try_join!(send, receive)?;
    sender.close().await
}
