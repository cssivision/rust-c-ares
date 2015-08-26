// This example uses get_sock() to find out which file descriptors c-ares
// wants us to listen on, and uses epoll() to satisfy those requirements.
extern crate c_ares;
extern crate nix;

use nix::sys::epoll::{
    epoll_create,
    epoll_ctl,
    epoll_wait,
    EpollEvent,
    EpollEventKind,
    EpollOp,
    EPOLLIN,
    EPOLLOUT,
};
use std::collections::HashSet;
use std::error::Error;
use std::os::unix::io::{
    AsRawFd,
    RawFd,
};

fn print_a_results(result: Result<c_ares::AResults, c_ares::AresError>) {
    match result {
        Err(e) => {
            println!("A lookup failed with error '{}'", e.description());
        }
        Ok(a_results) => {
            println!("Successful lookup...");
            println!("Hostname: {}", a_results.hostname());
            for alias in a_results.aliases() {
                println!("{:}", alias.alias());
            }
            for address in a_results.addresses() {
                println!("{:}", address.ip_address());
            }
        }
    }
}

fn print_aaaa_results(result: Result<c_ares::AAAAResults, c_ares::AresError>) {
    match result {
        Err(e) => {
            println!("AAAA lookup failed with error '{}'", e.description());
        }
        Ok(aaaa_results) => {
            println!("Successful lookup...");
            println!("Hostname: {}", aaaa_results.hostname());
            for alias in aaaa_results.aliases() {
                println!("{:}", alias.alias());
            }
            for address in aaaa_results.addresses() {
                println!("{:}", address.ip_address());
            }
        }
    }
}

fn print_srv_results(result: Result<c_ares::SRVResults, c_ares::AresError>) {
    match result {
        Err(e) => {
            println!("SRV lookup failed with error '{}'", e.description());
        }
        Ok(srv_results) => {
            println!("Successful SRV lookup...");
            for srv_result in &srv_results {
                println!("host: {} (port: {}), priority: {} weight: {}",
                         srv_result.host(),
                         srv_result.port(),
                         srv_result.priority(),
                         srv_result.weight());
            }
        }
    }
}

fn main() {
    // Create the c_ares::Channel.
    let mut options = c_ares::Options::new();
    options
        .set_flags(c_ares::flags::STAYOPEN)
        .set_timeout(500)
        .set_tries(3);
    let mut ares_channel = c_ares::Channel::new(options)
        .ok()
        .expect("Failed to create channel");

    // Set up some queries.
    ares_channel.query_a("apple.com", move |result| {
        println!("");
        print_a_results(result);
    });

    ares_channel.query_aaaa("google.com", move |result| {
        println!("");
        print_aaaa_results(result);
    });

    ares_channel.query_srv("_xmpp-server._tcp.gmail.com", move |result| {
        println!("");
        print_srv_results(result);
    });

    // Create an epoll file descriptor so that we can listen for events.
    let epoll = epoll_create().ok().expect("Failed to create epoll");
    let mut tracked_fds = HashSet::<RawFd>::new();
    loop {
        // Ask c-ares what file descriptors we should be listening on, and map
        // those requests onto the epoll file descriptor.
        let mut active = false;
        for (sock, readable, writable) in &ares_channel.get_sock() {
            let fd = sock.as_raw_fd();
            let mut interest = EpollEventKind::empty();
            if readable { interest = interest | EPOLLIN; }
            if writable { interest = interest | EPOLLOUT; }
            let event = EpollEvent {
                events: interest,
                data: fd as u64,
            };
            let op = if tracked_fds.insert(fd) {
                EpollOp::EpollCtlAdd
            } else {
                EpollOp::EpollCtlMod
            };
            epoll_ctl(epoll, op, fd, &event).ok().expect("epoll_ctl failed");
            active = true;
        }
        if !active { break }

        // Wait for something to happen.
        let empty_event = EpollEvent {
            events: EpollEventKind::empty(),
            data: 0,
        };
        let mut events = [empty_event; 2];
        let results = epoll_wait(epoll, &mut events, 500)
            .ok()
            .expect("epoll_wait failed");

        // Process whatever happened.
        match results {
            0 => {
                // No events - must be a timeout.  Tell c-ares about it.
                ares_channel.process_fd(
                    c_ares::SOCKET_BAD,
                    c_ares::SOCKET_BAD);
            },
            n => {
                // Sockets became readable or writable.  Tell c-ares about it.
                for event in &events[0..n] {
                    let active_fd = event.data as RawFd;
                    let readable_fd = if (event.events & EPOLLIN).is_empty() {
                        c_ares::SOCKET_BAD
                    } else {
                        c_ares::Socket(active_fd)
                    };
                    let writable_fd = if (event.events & EPOLLOUT).is_empty() {
                        c_ares::SOCKET_BAD
                    } else {
                        c_ares::Socket(active_fd)
                    };
                    ares_channel.process_fd(readable_fd, writable_fd);
                }
            }
        }
    }
}
