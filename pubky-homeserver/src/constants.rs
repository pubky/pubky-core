use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use pkarr::Keypair;


pub const DEFAULT_ADMIN_LISTEN_SOCKET: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6288));
pub const DEFAULT_PUBKY_TLS_LISTEN_SOCKET: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6287));
pub const DEFAULT_ICANN_HTTP_LISTEN_SOCKET: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6286));


pub fn default_keypair() -> Keypair {
    let secret_hex: &str = "0000000000000000000000000000000000000000000000000000000000000000"; // HEX
    let secret: [u8; 32] = hex::decode(secret_hex).expect("is always valid hex").try_into().expect("is always 32 bytes");
    Keypair::from_secret_key(&secret)
}

