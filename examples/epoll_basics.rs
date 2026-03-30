//! epoll basics — demonstrates the Linux kernel's epoll I/O event notification
//!
//! This example creates a Unix domain socket pair and uses epoll to wait for
//! data, demonstrating the same kernel mechanism that zbus uses internally
//! for D-Bus communication.
//!
//! Run via: cargo run --example epoll_basics

use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use nix::sys::socket::{socketpair, AddressFamily, SockFlag, SockType};
use nix::unistd::{read, write};
use std::os::fd::AsRawFd;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Linux Kernel epoll Demo ===\n");

    // --- Step 1: Create a Unix domain socket pair ---
    // Kernel syscall: socketpair(AF_UNIX, SOCK_STREAM, 0, fds)
    // This creates two connected sockets — like a bidirectional pipe.
    // The kernel allocates a socket buffer (sk_buff) for each direction.
    let (reader_fd, writer_fd) = socketpair(
        AddressFamily::Unix,
        SockType::Stream,
        None,
        SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
    )?;
    println!("1. Created socket pair: reader=fd{}, writer=fd{}",
        reader_fd.as_raw_fd(), writer_fd.as_raw_fd());
    println!("   Kernel: socketpair(AF_UNIX, SOCK_STREAM) → two connected fds\n");

    // --- Step 2: Create an epoll instance ---
    // Kernel syscall: epoll_create1(EPOLL_CLOEXEC)
    // Returns a file descriptor that represents an epoll interest list.
    // Internally the kernel creates an eventpoll struct with a red-black tree
    // (for the interest list) and a linked list (for ready events).
    let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;
    println!("2. Created epoll instance: fd{}", epoll.0.as_raw_fd());
    println!("   Kernel: allocates eventpoll struct with rb-tree + ready list\n");

    // --- Step 3: Register interest in the reader socket ---
    // Kernel syscall: epoll_ctl(epoll_fd, EPOLL_CTL_ADD, reader_fd, {EPOLLIN})
    // Adds the reader socket to the epoll interest list.
    // EPOLLIN means "notify me when there's data to read".
    // The kernel inserts an epitem into the red-black tree keyed by (fd, file*).
    let event = EpollEvent::new(EpollFlags::EPOLLIN, reader_fd.as_raw_fd() as u64);
    epoll.add(&reader_fd, event)?;
    println!("3. Registered reader fd with epoll (EPOLLIN)");
    println!("   Kernel: inserts epitem into rb-tree, installs callback on socket\n");

    // --- Step 4: Spawn a writer thread that sends data after a delay ---
    // Move the OwnedFd into the thread so it can write and then close it.
    let writer_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        let messages = [
            b"Hello from writer thread!" as &[u8],
            b"This is message 2",
            b"Final message",
        ];
        for (i, msg) in messages.iter().enumerate() {
            thread::sleep(Duration::from_millis(200));
            write(&writer_fd, msg).expect("write failed");
            println!("   [writer] Sent message {}: {:?}", i + 1, std::str::from_utf8(msg).unwrap());
        }
        // Close writer to signal EOF — dropping OwnedFd closes the fd
        drop(writer_fd);
        println!("   [writer] Closed socket (EOF)");
    });

    // --- Step 5: Event loop using epoll_wait ---
    // Kernel syscall: epoll_wait(epoll_fd, events, max_events, timeout_ms)
    // Blocks until one of the registered fds has an event, or timeout expires.
    //
    // What happens inside the kernel:
    //   1. Checks the ready list — if events already pending, returns immediately
    //   2. If empty, puts the calling thread to sleep on a wait queue
    //   3. When data arrives on the socket, the socket's callback wakes the epoll
    //   4. Kernel moves the epitem to the ready list
    //   5. epoll_wait returns with the ready events
    println!("4. Entering epoll event loop (waiting for data)...\n");

    let mut events = [EpollEvent::empty(); 4];
    let mut buf = [0u8; 256];
    let mut total_events = 0;

    loop {
        // Wait up to 2 seconds for events
        let n = epoll.wait(&mut events, 2000u16)?;

        if n == 0 {
            println!("   [epoll] Timeout — no events in 2s, exiting loop");
            break;
        }

        for event in &events[..n] {
            total_events += 1;
            let flags = event.events();
            println!("   [epoll] Event #{}: flags={:?}", total_events, flags);

            if flags.contains(EpollFlags::EPOLLIN) {
                match read(reader_fd.as_raw_fd(), &mut buf) {
                    Ok(0) => {
                        println!("   [epoll] Read 0 bytes — EOF (writer closed)");
                        println!("\n=== Event loop done: {} events processed ===", total_events);
                        writer_thread.join().unwrap();
                        return Ok(());
                    }
                    Ok(n) => {
                        let msg = std::str::from_utf8(&buf[..n]).unwrap_or("<binary>");
                        println!("   [epoll] Read {} bytes: {:?}", n, msg);
                    }
                    Err(nix::errno::Errno::EAGAIN) => {
                        println!("   [epoll] EAGAIN — no more data right now");
                    }
                    Err(e) => return Err(e.into()),
                }
            }

            if flags.contains(EpollFlags::EPOLLHUP) {
                println!("   [epoll] EPOLLHUP — peer closed connection");
                println!("\n=== Event loop done: {} events processed ===", total_events);
                writer_thread.join().unwrap();
                return Ok(());
            }
        }
    }

    writer_thread.join().unwrap();
    println!("\n=== Summary ===");
    println!("Kernel syscalls used:");
    println!("  socketpair()  → create connected Unix socket pair");
    println!("  epoll_create1() → create epoll instance (rb-tree + ready list)");
    println!("  epoll_ctl()   → register fd interest (EPOLLIN = readable)");
    println!("  epoll_wait()  → block until events ready (or timeout)");
    println!("  read()        → read data from socket buffer");
    println!("  write()       → write data to socket buffer");
    println!("\nThis is exactly what zbus does internally for D-Bus I/O.");

    Ok(())
}
