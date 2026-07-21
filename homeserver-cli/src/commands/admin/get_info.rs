use clap::Args;
use anyhow::Result;
use super::settings::AdminSettings;

#[derive(Args, Debug)]
pub struct GetInfoArgs {
}

pub fn run(settings: AdminSettings, _args: &GetInfoArgs) -> Result<()> {
    println!("{} {}",settings.endpoint , settings.password);
    println!("get_info_mock");
    Ok(())
}
