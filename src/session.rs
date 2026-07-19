// SPDX-License-Identifier: GPL-3.0-only

use smithay::reexports::{
    calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic},
    rustix,
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    os::unix::{
        io::{AsFd, BorrowedFd, FromRawFd, RawFd},
        net::UnixStream,
    },
};
use tracing::{error, warn};

use crate::state::{Common, State};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum Message {
    SetEnv { variables: HashMap<String, String> },
}

struct StreamWrapper {
    stream: UnixStream,
    header: [u8; 2],
    header_read: usize,
    buffer: Vec<u8>,
    read_bytes: usize,
}
impl AsFd for StreamWrapper {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }
}
impl From<UnixStream> for StreamWrapper {
    fn from(stream: UnixStream) -> StreamWrapper {
        StreamWrapper {
            stream,
            header: [0; 2],
            header_read: 0,
            buffer: Vec::new(),
            read_bytes: 0,
        }
    }
}

impl StreamWrapper {
    fn read_messages(&mut self, mut on_message: impl FnMut(&[u8])) -> io::Result<bool> {
        loop {
            if self.header_read < self.header.len() {
                match self.stream.read(&mut self.header[self.header_read..]) {
                    Ok(0) if self.header_read == 0 => return Ok(false),
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "session socket closed in a message header",
                        ));
                    }
                    Ok(read) => self.header_read += read,
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(true),
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                    Err(err) => return Err(err),
                }

                if self.header_read < self.header.len() {
                    continue;
                }

                let size = u16::from_ne_bytes(self.header) as usize;
                self.buffer.resize(size, 0);
                self.read_bytes = 0;
            }

            if self.buffer.is_empty() {
                on_message(&self.buffer);
                self.header_read = 0;
                continue;
            }

            match self.stream.read(&mut self.buffer[self.read_bytes..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "session socket closed in a message body",
                    ));
                }
                Ok(read) => self.read_bytes += read,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(true),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            }

            if self.read_bytes == self.buffer.len() {
                on_message(&self.buffer);
                self.header_read = 0;
                self.read_bytes = 0;
            }
        }
    }
}

fn handle_message(message: &[u8]) {
    match serde_json::from_slice::<Message>(message) {
        Ok(Message::SetEnv { .. }) => warn!("Got SetEnv from session? What is this?"),
        Err(err) => warn!(
            ?err,
            "Unknown session socket message, are you using incompatible cosmic-session and cosmic-comp versions?"
        ),
    }
}

unsafe fn set_cloexec(fd: RawFd) -> rustix::io::Result<()> {
    if fd == -1 {
        return Err(rustix::io::Errno::BADF);
    }
    let fd = unsafe { BorrowedFd::borrow_raw(fd) };
    let flags = rustix::io::fcntl_getfd(fd)?;
    rustix::io::fcntl_setfd(fd, flags | rustix::io::FdFlags::CLOEXEC)
}

pub fn get_env(common: &Common) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    env.insert(
        String::from("WAYLAND_DISPLAY"),
        common
            .socket
            .clone()
            .into_string()
            .map_err(|_| anyhow!("wayland socket is no valid utf-8 string?"))?,
    );
    if let Some(display) = common.xwayland_state.as_ref().map(|s| s.display) {
        env.insert(String::from("DISPLAY"), format!(":{}", display));
    }
    Ok(env)
}

pub fn setup_socket() -> Result<()> {
    if let Ok(fd_num) = std::env::var("COSMIC_SESSION_SOCK")
        && let Ok(fd) = fd_num.parse::<RawFd>()
    {
        let res = unsafe { set_cloexec(fd) }.with_context(|| "Failed to setup session socket");
        if res.is_err() {
            unsafe { rustix::io::close(fd) };
        }
        res
    } else {
        Ok(())
    }
}

pub fn run_socket(handle: LoopHandle<State>, common: &Common) -> Result<()> {
    if let Ok(fd_num) = std::env::var("COSMIC_SESSION_SOCK") {
        if let Ok(fd) = fd_num.parse::<RawFd>() {
            let mut session_socket = unsafe { UnixStream::from_raw_fd(fd) };

            let env = get_env(common)?;
            let message = serde_json::to_string(&Message::SetEnv { variables: env })
                .with_context(|| "Failed to encode environment variables into json")?;
            let bytes = message.into_bytes();
            let len = (bytes.len() as u16).to_ne_bytes();
            session_socket
                .write_all(&len)
                .with_context(|| "Failed to write message len")?;
            session_socket
                .write_all(&bytes)
                .with_context(|| "Failed to write message bytes")?;
            session_socket
                .set_nonblocking(true)
                .with_context(|| "Failed to make the cosmic session socket nonblocking")?;

            handle
                .insert_source(
                    Generic::new(
                        StreamWrapper::from(session_socket),
                        Interest::READ,
                        Mode::Level,
                    ),
                    move |_, stream, _state| {
                        // SAFETY: We don't drop the stream!
                        let stream = unsafe { stream.get_mut() };

                        match stream.read_messages(handle_message) {
                            Ok(true) => Ok(PostAction::Continue),
                            Ok(false) => Ok(PostAction::Remove),
                            Err(err) => {
                                error!(?err, "Error reading from session socket");
                                Ok(PostAction::Remove)
                            }
                        }
                    },
                )
                .with_context(|| "Failed to init the cosmic session socket source")?;
        } else {
            error!(socket = fd_num, "COSMIC_SESSION_SOCK is no valid RawFd.");
        }
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = u16::try_from(payload.len()).unwrap().to_ne_bytes().to_vec();
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn reads_fragmented_messages_without_blocking() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let mut stream = StreamWrapper::from(reader);
        let payload: &[u8] = br#"{"message":"set_env","variables":{"A":"B"}}"#;
        let framed = frame(payload);
        let mut messages = Vec::new();

        writer.write_all(&framed[..1]).unwrap();
        assert!(
            stream
                .read_messages(|message| messages.push(message.to_vec()))
                .unwrap()
        );
        assert!(messages.is_empty());

        writer.write_all(&framed[1..5]).unwrap();
        assert!(
            stream
                .read_messages(|message| messages.push(message.to_vec()))
                .unwrap()
        );
        assert!(messages.is_empty());

        writer.write_all(&framed[5..]).unwrap();
        assert!(
            stream
                .read_messages(|message| messages.push(message.to_vec()))
                .unwrap()
        );
        assert_eq!(messages, [payload]);
    }

    #[test]
    fn reads_multiple_messages_from_one_ready_event() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let mut stream = StreamWrapper::from(reader);
        let first: &[u8] = br#"{"message":"set_env","variables":{}}"#;
        let second: &[u8] = br#"{"message":"set_env","variables":{"A":"B"}}"#;
        let mut bytes = frame(first);
        bytes.extend(frame(second));
        writer.write_all(&bytes).unwrap();

        let mut messages = Vec::new();
        assert!(
            stream
                .read_messages(|message| messages.push(message.to_vec()))
                .unwrap()
        );
        assert_eq!(messages, [first, second]);
    }
}
