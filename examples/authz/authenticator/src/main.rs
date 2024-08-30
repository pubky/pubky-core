use std::io;

use keyring::Entry;
use pubky_common::crypto::Keypair;

const SERVICE_NAME: &str = "pubky";

fn main() -> anyhow::Result<()> {
    println!("Enter the alias for your keypair in your operating system secure storage:");
    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    name = name.trim_end().to_lowercase();

    let entry = Entry::new(SERVICE_NAME, &name)?;

    let keypair = match entry.get_secret() {
        Ok(secret_key) => {
            let secret_key: &[u8; 32] = secret_key
                .as_slice()
                .try_into()
                .expect("Invalid secret_key");
            let keypair = Keypair::from_secret_key(&secret_key);

            println!("\nFound secret_key for Pubky {}", keypair.public_key());

            keypair
        }
        Err(error) => {
            let keypair = Keypair::random();

            println!(
                "\n{}\nGenerated new Pubky {}",
                error.to_string(),
                keypair.public_key()
            );

            loop {
                println!("\nStore the new Pubky keypair in operating system secure storage?[y/n]");
                let mut choice = String::new();
                io::stdin().read_line(&mut choice)?;

                match choice.as_str() {
                    "y\n" => {
                        entry.set_secret(&keypair.secret_key())?;

                        break;
                    }
                    "n\n" => {
                        return Ok(());
                    }
                    _ => {}
                };
            }

            keypair
        }
    };
    dbg!(keypair);

    // entry.set_password("topS3cr3tP4$$w0rd")?;
    // let password = entry.get_password()?;
    // println!("My password is '{}'", password);
    // entry.delete_credential()?;
    // Ok(())

    Ok(())
}

// fn read_file_content<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
//     let mut file = File::open(path)?;
//     let mut content = Vec::new();
//     file.read_to_end(&mut content)?;
//     Ok(content)
// }

// fn decrypt_content(content: &[u8], password: &str) -> Result<String, &'static str> {
//     // Create a key and IV (Initialization Vector) from the password
//     let key = GenericArray::from_slice(password.as_bytes()); // Example key derivation
//     let iv = GenericArray::from_slice(b"unique_iv_16bytes"); // Replace with proper IV derivation
//
//     let cipher = Aes256::new(key, iv);
//
//     // Decrypt the content using the cipher
//     let mut buffer = content.to_vec();
//     cipher
//         .decrypt_padded_mut::<Pkcs7>(&mut buffer)
//         .map_err(|_| "Decryption failed")?;
//
//     // Convert decrypted bytes to string
//     String::from_utf8(buffer).map_err(|_| "Failed to convert decrypted content to string")
// }
