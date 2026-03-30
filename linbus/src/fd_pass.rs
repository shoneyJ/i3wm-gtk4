use std::os::fd::{OwnedFd, RawFd, FromRawFd};
use std::io::IoSlice;
use nix::sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags};

use crate::error::LinbusError;

/// Send data with optional file descriptors via SCM_RIGHTS.
pub fn send_with_fds(fd: RawFd, data: &[u8], fds: &[RawFd]) -> Result<usize, LinbusError> {
    let iov = [IoSlice::new(data)];
    let sent = if fds.is_empty() {
        sendmsg::<()>(fd, &iov, &[], MsgFlags::empty(), None)?
    } else {
        let cmsg = [ControlMessage::ScmRights(fds)];
        sendmsg::<()>(fd, &iov, &cmsg, MsgFlags::empty(), None)?
    };
    Ok(sent)
}

/// Receive data and any file descriptors passed via SCM_RIGHTS.
pub fn recv_with_fds(fd: RawFd, buf: &mut [u8]) -> Result<(usize, Vec<OwnedFd>), LinbusError> {
    let mut iov = [std::io::IoSliceMut::new(buf)];
    let mut cmsg_buf = nix::cmsg_space!([RawFd; 4]);

    let msg = recvmsg::<()>(fd, &mut iov, Some(&mut cmsg_buf), MsgFlags::empty())?;

    let mut fds = Vec::new();
    for cmsg in msg.cmsgs()? {
        if let ControlMessageOwned::ScmRights(received_fds) = cmsg {
            for raw_fd in received_fds {
                // SAFETY: these fds were passed to us via SCM_RIGHTS, we now own them
                fds.push(unsafe { OwnedFd::from_raw_fd(raw_fd) });
            }
        }
    }

    Ok((msg.bytes, fds))
}
