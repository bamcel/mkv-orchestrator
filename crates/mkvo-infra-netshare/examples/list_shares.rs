//! Live check against a real server: `cargo run -p mkvo-infra-netshare --example list_shares -- 192.168.1.100`

fn main() {
    let server = std::env::args()
        .nth(1)
        .expect("usage: list_shares <server>");
    match mkvo_infra_netshare::list_server_shares(&server) {
        Ok(shares) if shares.is_empty() => println!("no disk shares on {server}"),
        Ok(shares) => {
            for share in shares {
                println!("{:<24} {}", share.name, share.path);
            }
        }
        Err(error) => println!("failed: {error}"),
    }
}
