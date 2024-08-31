// use std::io;

use anyhow::Result;
// use clap::{App, Arg, SubCommand};
// use dialoguer::{Input, Select};

use keyring_search::{Limit, List, Search};

// use keyring::Entry;
// use pubky_common::crypto::Keypair;

const SERVICE_NAME: &str = "pubky";

fn main() -> Result<()> {
    let result = Search::new().unwrap().by_service(SERVICE_NAME).unwrap();

    // let list: Vec<_> = result
    //     .values()
    //     .map(|v| {
    //         dbg!(&v);
    //         v.get("acct")
    //     })
    //     .filter(|acc| acc.is_some())
    //     .collect();

    let list = List::list_credentials(&Search::new().unwrap().by_service(SERVICE_NAME), Limit::All);

    dbg!(list);

    // println!("Enter the alias for your keypair in your operating system secure storage:");
    // let mut name = String::new();
    // io::stdin().read_line(&mut name)?;
    // name = name.trim_end().to_lowercase();
    //
    // let entry = Entry::new(SERVICE_NAME, &name)?;
    //
    // let keypair = match entry.get_secret() {
    //     Ok(secret_key) => {
    //         let secret_key: &[u8; 32] = secret_key
    //             .as_slice()
    //             .try_into()
    //             .expect("Invalid secret_key");
    //         let keypair = Keypair::from_secret_key(&secret_key);
    //
    //         println!("\nFound secret_key for Pubky {}", keypair.public_key());
    //
    //         keypair
    //     }
    //     Err(error) => {
    //         let keypair = Keypair::random();
    //
    //         println!(
    //             "\n{}\nGenerated new Pubky {}",
    //             error.to_string(),
    //             keypair.public_key()
    //         );
    //
    //         loop {
    //             println!("\nStore the new Pubky keypair in operating system secure storage?[y/n]");
    //             let mut choice = String::new();
    //             io::stdin().read_line(&mut choice)?;
    //
    //             match choice.as_str() {
    //                 "y\n" => {
    //                     entry.set_secret(&keypair.secret_key())?;
    //
    //                     break;
    //                 }
    //                 "n\n" => {
    //                     return Ok(());
    //                 }
    //                 _ => {}
    //             };
    //         }
    //
    //         keypair
    //     }
    // };
    // dbg!(keypair);

    Ok(())
}
