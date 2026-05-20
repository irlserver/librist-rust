//! IP-over-RIST tunnel example.
//!
//! Opens a TUN device and bridges it to a RIST link, so raw IP packets are
//! forwarded over RIST. Run the receiver on one host and the sender on another
//! (or both locally with two TUN devices in different subnets).
//!
//! Requires root/admin privileges to open and configure the TUN device.
//!
//! Receiver (writes incoming RIST data to the TUN device):
//!   sudo cargo run --example tun_tunnel -- recv rist://@0.0.0.0:5000 10.9.0.1
//!
//! Sender (reads IP packets from the TUN device, sends over RIST):
//!   sudo cargo run --example tun_tunnel -- send rist://192.168.1.50:5000 10.9.0.2
//!
//! Then ping across the tunnel: `ping 10.9.0.1` from the sender host.

use librist::{DataFdFlags, Profile, RistReceiver, RistSender, Tun};
use std::env;
use std::time::Duration;

const TUN_MTU: u32 = 1400;

fn main() -> librist::Result<()> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <send|recv> <rist-url> <tun-ip>", args[0]);
        eprintln!("  send  rist://host:port 10.9.0.2");
        eprintln!("  recv  rist://@0.0.0.0:port 10.9.0.1");
        std::process::exit(1);
    }
    let mode = args[1].as_str();
    let url = args[2].as_str();
    let tun_ip = args[3].as_str();

    // Open and configure the TUN device. The Tun stays alive for the whole
    // session: librist borrows the fd but never owns it, so dropping it early
    // would pull the rug out from under the internal I/O thread.
    let tun = Tun::open(None)?;
    println!("opened TUN device: {}", tun.name());
    tun.set_ip(tun_ip, 24)?;
    tun.set_mtu(TUN_MTU)?;
    tun.bring_up()?;
    println!("configured {} as {}/24 mtu {}", tun.name(), tun_ip, TUN_MTU);

    match mode {
        "send" => run_sender(url, &tun)?,
        "recv" => run_receiver(url, &tun)?,
        other => {
            eprintln!("unknown mode: {other}");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn run_sender(url: &str, tun: &Tun) -> librist::Result<()> {
    let sender = RistSender::builder().profile(Profile::Main).build()?;
    sender.add_peer(url)?;
    // Feed the TUN fd into the sender before starting. TUN framing is handled
    // by librist via the flag.
    sender.set_data_fd(tun.as_raw_fd(), TUN_MTU as usize, DataFdFlags::TUN)?;
    sender.start()?;
    println!(
        "tunnel sender running on {url}, forwarding {} -> RIST",
        tun.name()
    );

    loop {
        std::thread::sleep(Duration::from_secs(5));
        let s = sender.data_fd_stats()?;
        println!("tx: {} packets / {} bytes", s.tx_packets, s.tx_bytes);
    }
}

fn run_receiver(url: &str, tun: &Tun) -> librist::Result<()> {
    let receiver = RistReceiver::builder().profile(Profile::Main).build()?;
    receiver.add_peer(url)?;
    // Write received RIST data straight to the TUN device (lowest-latency path).
    receiver.set_data_fd(tun.as_raw_fd(), DataFdFlags::TUN)?;
    receiver.start()?;
    println!(
        "tunnel receiver running on {url}, forwarding RIST -> {}",
        tun.name()
    );

    loop {
        std::thread::sleep(Duration::from_secs(5));
        let s = receiver.data_fd_stats()?;
        println!("rx: {} packets / {} bytes", s.rx_packets, s.rx_bytes);
    }
}
