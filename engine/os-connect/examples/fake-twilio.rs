//! A fake Twilio account on loopback, for the end-to-end demo
//! (apps/desktop/e2e/phone-calls.mjs). Never a real account or call.
//!
//!   cargo run -p os-connect --features mock --example fake-twilio -- <port> <account> <key> <secret> <caller-id>

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [port, account, key, secret, caller] = args.as_slice() else {
        eprintln!("usage: fake-twilio <port> <account sid> <key sid> <secret> <caller id>");
        std::process::exit(2);
    };
    let fake = os_connect::mock::fake::FakeTwilio::start_on(
        &format!("127.0.0.1:{port}"),
        account,
        key,
        secret,
        &[caller.as_str()],
    );
    println!("fake Twilio on {}", fake.server.base);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
