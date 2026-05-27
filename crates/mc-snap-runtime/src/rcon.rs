use anyhow::{bail, Context, Result};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TYPE_AUTH: i32 = 3;
const TYPE_EXEC: i32 = 2;
const TYPE_AUTH_RESPONSE: i32 = 2;
const TYPE_RESPONSE: i32 = 0;

pub struct Rcon {
    stream: TcpStream,
    next_id: i32,
}

impl Rcon {
    pub async fn connect(addr: &str, password: &str) -> Result<Self> {
        let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .context("rcon connect timeout")?
            .with_context(|| format!("connecting to rcon {addr}"))?;
        let mut me = Self { stream, next_id: 1 };
        me.auth(password).await?;
        Ok(me)
    }

    async fn auth(&mut self, password: &str) -> Result<()> {
        let id = self.alloc_id();
        self.write_packet(id, TYPE_AUTH, password.as_bytes()).await?;
        let (rid, rtype, _body) = self.read_packet().await?;
        if rtype != TYPE_AUTH_RESPONSE {
            bail!("unexpected rcon auth response type {rtype}");
        }
        if rid == -1 {
            bail!("rcon auth failed: bad password");
        }
        Ok(())
    }

    pub async fn exec(&mut self, cmd: &str) -> Result<String> {
        let id = self.alloc_id();
        self.write_packet(id, TYPE_EXEC, cmd.as_bytes()).await?;
        let (_rid, rtype, body) = self.read_packet().await?;
        if rtype != TYPE_RESPONSE {
            bail!("unexpected rcon exec response type {rtype}");
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    fn alloc_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    async fn write_packet(&mut self, id: i32, ptype: i32, body: &[u8]) -> Result<()> {
        let length = (4 + 4 + body.len() + 2) as i32;
        let mut buf = Vec::with_capacity(4 + length as usize);
        buf.extend_from_slice(&length.to_le_bytes());
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&ptype.to_le_bytes());
        buf.extend_from_slice(body);
        buf.push(0);
        buf.push(0);
        self.stream.write_all(&buf).await?;
        Ok(())
    }

    async fn read_packet(&mut self) -> Result<(i32, i32, Vec<u8>)> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let length = i32::from_le_bytes(len_buf);
        if !(10..=4096).contains(&length) {
            bail!("rcon packet length out of range: {length}");
        }
        let mut payload = vec![0u8; length as usize];
        self.stream.read_exact(&mut payload).await?;
        let id = i32::from_le_bytes(payload[0..4].try_into().unwrap());
        let ptype = i32::from_le_bytes(payload[4..8].try_into().unwrap());
        let body_end = payload.len().saturating_sub(2);
        let body = payload[8..body_end].to_vec();
        Ok((id, ptype, body))
    }
}
