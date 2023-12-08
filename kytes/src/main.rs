mod passphrase;

use passphrase::generate_4words_passphrase;

fn main() {
    println!("Hello, world!");
    println!("{}", generate_4words_passphrase());
}
